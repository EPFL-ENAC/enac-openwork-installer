use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::types::{AppError, CreatedKey, PortalForm, PortalKeyChoice};
use crate::util;

pub(crate) const PORTAL_BASE: &str = "https://portal.rcp.epfl.ch";
pub(crate) const KEYS_PATH: &str = "/aiaas/keys";
// Live check on 2026-11-02 against portal 1.9.2: the key pages are server-rendered
// Django. The list comes from the documented JSON API, the secret is revealed by the
// htmx endpoint the keys page itself posts to, and the create form is a modal.
const KEYS_API_PATH: &str = "/aiaas/api/v1/keys";
const NEW_KEY_MODAL_PATH: &str = "/aiaas/modal_key_new";
const SHOW_SECRET_PATH: &str = "/aiaas/showsecret";
// Same default name as DEFAULT_KEY_NAME in src/lib/key-choice.ts.
const OPEN_WORK_KEY_NAME: &str = "OpenWork";
const LOGIN_WINDOW_LABEL: &str = "portal-login";
const LOGIN_TIMEOUT_SECS: u64 = 600;
const COOKIE_POLL_MS: u64 = 1000;
const COOKIE_POLL_MAX_ERRORS: u32 = 15;
// A rejected sessionid is re-probed this often, in case the portal keeps the same
// session key across the Entra ID callback. Expressed in cookie-poll ticks.
const SESSION_REPROBE_TICKS: u32 = 5;
const PORTAL_HOST: &str = "portal.rcp.epfl.ch";

fn is_portal_page(url: &reqwest::Url) -> bool {
    url.host_str() == Some(PORTAL_HOST) && url.path().starts_with("/aiaas")
}

const EXCERPT_LIMIT: usize = 400;
// A portal key or a CSRF token is a long run of token characters. Masking every such
// run keeps a page excerpt safe even when the page echoes a secret this flow never saw.
const TOKEN_RUN_MIN: usize = 24;

/// Status, path and a masked excerpt of the page, so a trace line names what the
/// endpoint really answered without a debugger.
pub(crate) fn endpoint_detail(status: u16, url: &str, body: &str, secrets: &[String]) -> String {
    let path = tauri::Url::parse(url)
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|_| url.to_string());
    let excerpt = page_excerpt(body, secrets);
    if excerpt.is_empty() {
        format!("HTTP {status} on {path}")
    } else {
        format!("HTTP {status} on {path}: \"{excerpt}\"")
    }
}

/// Visible text of a portal page: script and style blocks dropped, whitespace
/// collapsed, session secrets and token-shaped runs masked.
pub(crate) fn page_excerpt(body: &str, secrets: &[String]) -> String {
    let stripped = strip_block(&strip_block(body, "script"), "style");
    let document = scraper::Html::parse_document(&stripped);
    let text = document.root_element().text().collect::<String>();
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let masked = sanitize(&collapsed, secrets);
    let mut excerpt: String = masked.chars().take(EXCERPT_LIMIT).collect();
    if masked.chars().count() > EXCERPT_LIMIT {
        excerpt.push_str("...");
    }
    excerpt
}

/// Masks the session secrets this flow knows and every token-shaped run, so a page
/// body can be shown or dumped without leaking a credential.
fn sanitize(text: &str, secrets: &[String]) -> String {
    let mut masked = text.to_string();
    for secret in secrets
        .iter()
        .map(|secret| secret.trim())
        .filter(|secret| !secret.is_empty())
    {
        masked = masked.replace(secret, util::MASK);
    }
    mask_token_shapes(&masked)
}

fn strip_block(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find(&open) {
        let start = cursor + found;
        out.push_str(&html[cursor..start]);
        let Some(end) = lower[start..].find(&close) else {
            return out;
        };
        let after_close = start + end;
        let Some(gt) = lower[after_close..].find('>') else {
            return out;
        };
        cursor = after_close + gt + 1;
    }
    out.push_str(&html[cursor..]);
    out
}

fn mask_token_shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '+' | '/' | '=') {
            run.push(character);
            continue;
        }
        push_run(&mut out, &run);
        run.clear();
        out.push(character);
    }
    push_run(&mut out, &run);
    out
}

fn push_run(out: &mut String, run: &str) {
    if run.chars().count() >= TOKEN_RUN_MIN {
        out.push_str(util::MASK);
    } else {
        out.push_str(run);
    }
}

// Django CSRF middleware compares the form token against the csrftoken cookie
// by default, so requests must replay both cookies captured in the webview.
fn session_cookie_header(sessionid: &str, csrftoken: Option<&str>) -> String {
    match csrftoken {
        Some(csrftoken) => format!("sessionid={sessionid}; csrftoken={csrftoken}"),
        None => format!("sessionid={sessionid}"),
    }
}

/// What the sign-in poller does with the cookies it just read from the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PollAction {
    /// No sessionid yet, or the current one was rejected too recently.
    Wait,
    /// The sessionid is new (or due for a retry): check it against the portal.
    Verify,
}

/// Login gate for the sign-in window.
///
/// The portal mints an anonymous `sessionid` on its redirect to Entra ID, before the
/// user types anything, so the presence of the cookie proves nothing. The session
/// only counts once `GET /aiaas/keys` answers a portal page with that cookie.
#[derive(Debug, Default)]
pub(crate) struct LoginTracker {
    probed: Option<String>,
    retry_in: u32,
}

impl LoginTracker {
    pub(crate) fn on_cookies(&mut self, sessionid: Option<&str>) -> PollAction {
        let Some(sessionid) = sessionid else {
            return PollAction::Wait;
        };
        if self.probed.as_deref() == Some(sessionid) && self.retry_in > 0 {
            self.retry_in -= 1;
            return PollAction::Wait;
        }
        self.probed = Some(sessionid.to_string());
        self.retry_in = SESSION_REPROBE_TICKS;
        PollAction::Verify
    }
}

/// True only for a signed-in portal session: the keys page answers 200 for a logged-in
/// user and a 302 to /accounts/login for the anonymous session that already owns a
/// cookie. Redirects stay off so a probe never pulls the Entra ID page.
async fn session_is_authenticated(client: &reqwest::Client, cookie_header: &str) -> bool {
    client
        .get(format!("{PORTAL_BASE}{KEYS_PATH}"))
        .header(reqwest::header::COOKIE, cookie_header)
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

// Security: sessionid, csrf, and token live here in memory only. Never log them,
// never persist them, never put them into AppError messages.
#[derive(Clone, Default)]
pub struct PortalState(Arc<Mutex<PortalSession>>);

#[derive(Default)]
struct PortalSession {
    sessionid: Option<String>,
    csrftoken: Option<String>,
    csrf: Option<String>,
    token: Option<String>,
    form_url: Option<String>,
    units: Vec<PortalUnit>,
    login_generation: u64,
}

impl PortalState {
    fn lock(&self) -> MutexGuard<'_, PortalSession> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn csrf(&self) -> Option<String> {
        self.lock().csrf.clone()
    }

    // Spec keeps the API token in backend state; nothing reads it outside tests today
    // (the UI receives it from the portal_create_key return). Kept as the state read API.
    #[allow(dead_code)]
    fn token(&self) -> Option<String> {
        self.lock().token.clone()
    }

    fn form_url(&self) -> Option<String> {
        self.lock().form_url.clone()
    }

    fn units(&self) -> Vec<PortalUnit> {
        self.lock().units.clone()
    }

    fn csrftoken(&self) -> Option<String> {
        self.lock().csrftoken.clone()
    }

    fn next_login_generation(&self) -> u64 {
        let mut session = self.lock();
        session.login_generation += 1;
        session.login_generation
    }

    fn login_generation(&self) -> u64 {
        self.lock().login_generation
    }

    fn set_session(&self, sessionid: &str, csrftoken: Option<&str>) {
        let mut session = self.lock();
        session.sessionid = Some(sessionid.to_string());
        session.csrftoken = csrftoken.map(str::to_string);
    }

    fn cookie_header(&self) -> Option<String> {
        let session = self.lock();
        let sessionid = session.sessionid.as_deref()?;
        Some(session_cookie_header(
            sessionid,
            session.csrftoken.as_deref(),
        ))
    }

    /// Every value that must never reach a trace line: the cookies replayed on each
    /// request plus the two tokens the portal hands out.
    fn secrets(&self) -> Vec<String> {
        let session = self.lock();
        [
            session.sessionid.as_deref(),
            session.csrftoken.as_deref(),
            session.csrf.as_deref(),
            session.token.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::to_string)
        .collect()
    }

    fn set_form(&self, csrf: &str, csrftoken: Option<&str>, form_url: &str, units: &[PortalUnit]) {
        let mut session = self.lock();
        session.csrf = Some(csrf.to_string());
        if csrftoken.is_some() {
            session.csrftoken = csrftoken.map(str::to_string);
        }
        session.form_url = Some(form_url.to_string());
        session.units = units.to_vec();
    }

    fn set_token(&self, token: &str) {
        self.lock().token = Some(token.to_string());
    }

    fn clear(&self) {
        let mut session = self.lock();
        session.login_generation += 1;
        *session = PortalSession {
            login_generation: session.login_generation,
            ..PortalSession::default()
        };
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PortalLoginEvent {
    ok: bool,
    error: Option<String>,
}

fn emit_login_result<R: tauri::Runtime>(app: &AppHandle<R>, ok: bool, error: Option<&str>) {
    let _ = app.emit(
        "portal-login",
        PortalLoginEvent {
            ok,
            error: error.map(str::to_string),
        },
    );
}

#[tauri::command]
pub async fn portal_start_login<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, PortalState>,
) -> Result<(), AppError> {
    let login_url: tauri::Url = format!("{PORTAL_BASE}{KEYS_PATH}")
        .parse()
        .map_err(|error| AppError::new("parse", format!("The portal URL is invalid: {error}")))?;
    let probe_client = portal_client(reqwest::redirect::Policy::none())?;
    if let Some(existing) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = existing.destroy();
    }
    WebviewWindowBuilder::new(
        &app,
        LOGIN_WINDOW_LABEL,
        WebviewUrl::External(login_url.clone()),
    )
    .title("EPFL portal sign in")
    .inner_size(960.0, 720.0)
    // In-memory cookie store on all three desktop engines (wry keeps the cookie API
    // pointed at the same ephemeral store), so the portal session never hits disk.
    .incognito(true)
    .build()
    .map_err(|error| AppError::new("io", format!("Could not open the sign-in window: {error}")))?;

    let login_state = state.inner().clone();
    let poll_app = app.clone();
    let generation = state.next_login_generation();
    tauri::async_runtime::spawn(async move {
        let deadline = Instant::now() + Duration::from_secs(LOGIN_TIMEOUT_SECS);
        let mut cookie_errors = 0u32;
        let mut tracker = LoginTracker::default();
        loop {
            tokio::time::sleep(Duration::from_millis(COOKIE_POLL_MS)).await;
            if login_state.login_generation() != generation {
                return;
            }
            let Some(window) = poll_app.get_webview_window(LOGIN_WINDOW_LABEL) else {
                // The window can also be briefly absent while this command rebuilds it;
                // re-check once before reporting the window as closed.
                tokio::time::sleep(Duration::from_millis(COOKIE_POLL_MS * 2)).await;
                if login_state.login_generation() != generation {
                    return;
                }
                if poll_app.get_webview_window(LOGIN_WINDOW_LABEL).is_none() {
                    emit_login_result(
                        &poll_app,
                        false,
                        Some("The sign-in window was closed before the portal session appeared."),
                    );
                    return;
                }
                continue;
            };
            if Instant::now() >= deadline {
                if login_state.login_generation() == generation {
                    let _ = window.destroy();
                    emit_login_result(
                        &poll_app,
                        false,
                        Some("The portal sign-in timed out. Try again."),
                    );
                }
                return;
            }
            // cookies_for_url deadlocks when called from a sync command on Windows (wry#583),
            // so this poll always runs on the async runtime, never on the main thread.
            let Ok(cookies) = window.cookies_for_url(login_url.clone()) else {
                cookie_errors += 1;
                if cookie_errors >= COOKIE_POLL_MAX_ERRORS {
                    if login_state.login_generation() == generation {
                        let _ = window.destroy();
                        emit_login_result(
                            &poll_app,
                            false,
                            Some("The sign-in window cannot expose cookies on this platform. Use the manual paste fallback."),
                        );
                    }
                    return;
                }
                continue;
            };
            cookie_errors = 0;
            let cookie_strings: Vec<String> = cookies
                .iter()
                .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
                .collect();
            let sessionid = match extract_sessionid(&cookie_strings) {
                Some(sessionid) => sessionid,
                None => continue,
            };
            if tracker.on_cookies(Some(&sessionid)) == PollAction::Wait {
                continue;
            }
            // The anonymous sessionid from the Entra ID redirect lands here too: keep the
            // window open until the portal accepts the cookie, so the user can finish
            // the login instead of losing the window after one second.
            let csrftoken = extract_cookie_value(&cookie_strings, "csrftoken");
            let cookie_header = session_cookie_header(&sessionid, csrftoken.as_deref());
            if !session_is_authenticated(&probe_client, &cookie_header).await {
                continue;
            }
            if login_state.login_generation() == generation {
                login_state.set_session(&sessionid, csrftoken.as_deref());
                emit_login_result(&poll_app, true, None);
                let _ = window.close();
            }
            return;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn portal_get_form(state: State<'_, PortalState>) -> Result<PortalForm, AppError> {
    let form = load_portal_form(state.inner()).await?;
    Ok(PortalForm {
        units: form.units.into_iter().map(|unit| unit.label).collect(),
        existing_open_work_key: form.existing_open_work_key,
    })
}

#[tauri::command]
pub async fn portal_list_keys(
    state: State<'_, PortalState>,
) -> Result<Vec<PortalKeyChoice>, AppError> {
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new(
            "portal-auth",
            "Sign in to the portal before listing the keys.",
        )
    })?;
    let client = portal_client(reqwest::redirect::Policy::default())?;
    let secrets = state.secrets();
    Ok(selectable_key_choices(
        &list_keys(&client, &cookie_header, &secrets).await?,
    ))
}

/// Reuses one key the user picked. The chosen id is checked against the same selectable
/// list the card showed, so an application or legacy key is refused instead of revealed.
#[tauri::command]
pub async fn portal_use_key(
    id: i64,
    state: State<'_, PortalState>,
) -> Result<CreatedKey, AppError> {
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new("portal-auth", "Sign in to the portal before reusing a key.")
    })?;
    let client = portal_client(reqwest::redirect::Policy::default())?;
    let secrets = state.secrets();
    let name = selectable_key_choices(&list_keys(&client, &cookie_header, &secrets).await?)
        .into_iter()
        .find(|key| key.id == id)
        .map(|key| key.name)
        .ok_or_else(|| {
            AppError::new(
                "portal-key-missing",
                "The portal does not offer that key for reuse.",
            )
        })?;
    let token = reveal_secret(
        &client,
        &cookie_header,
        state.csrftoken().as_deref(),
        id,
        &secrets,
    )
    .await?;
    state.set_token(&token);
    Ok(CreatedKey { name, token })
}

#[tauri::command]
pub async fn portal_create_key(
    name: String,
    unit: Option<String>,
    state: State<'_, PortalState>,
) -> Result<CreatedKey, AppError> {
    if state.csrf().is_none() || state.form_url().is_none() {
        load_portal_form(state.inner()).await?;
    }
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new(
            "portal-auth",
            "Sign in to the portal before creating a key.",
        )
    })?;
    let client = portal_client(reqwest::redirect::Policy::default())?;
    let secrets = state.secrets();
    // The portal reveals a stored secret but a name never appears twice, so an existing
    // key is reused instead of refused.
    if let Some(key) =
        find_reusable_key(&list_keys(&client, &cookie_header, &secrets).await?, &name)
    {
        let id = key.id;
        let token = reveal_secret(
            &client,
            &cookie_header,
            state.csrftoken().as_deref(),
            id,
            &secrets,
        )
        .await?;
        state.set_token(&token);
        return Ok(CreatedKey { name, token });
    }
    let csrf = state
        .csrf()
        .ok_or_else(|| AppError::new("parse", "The key creation form did not load."))?;
    let form_url = state
        .form_url()
        .ok_or_else(|| AppError::new("parse", "The key creation form did not load."))?;
    let units = state.units();
    let unit = match unit
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(picked) => units
            .iter()
            .find(|unit| unit.label == picked || unit.value == picked)
            .cloned(),
        None => units.first().cloned(),
    }
    .ok_or_else(|| AppError::new("parse", "The portal form offers no unit for the key."))?;

    let response = client
        .post(&form_url)
        .header(reqwest::header::COOKIE, cookie_header.as_str())
        .header(reqwest::header::REFERER, form_url.as_str())
        .header("HX-Request", "true")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(build_create_body(&csrf, &name, &unit.value))
        .send()
        .await
        .map_err(|error| {
            AppError::new(
                "network",
                format!("The key creation request failed: {error}"),
            )
        })?;
    let status = response.status();
    let final_url = response.url().clone();
    let body = response.text().await.map_err(|error| {
        AppError::new(
            "network",
            format!("Could not read the key creation response: {error}"),
        )
    })?;
    let page = FetchedPage {
        status: status.as_u16(),
        final_url: final_url.to_string(),
        body,
        set_cookies: Vec::new(),
    };
    let detail = page.detail(&state.secrets());

    if !is_portal_page(&final_url) {
        return Err(portal_failure(
            "portal-auth",
            "The portal ended the session during key creation. Sign in again.",
            &detail,
        ));
    }
    if let Some(message) = extract_error_message(&page.body) {
        return Err(portal_failure("portal-refused", message, &detail));
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(portal_failure(
            "portal-auth",
            "The portal rejected the key creation. The session or CSRF token expired, so load the form and try again.",
            &detail,
        ));
    }
    if status.is_server_error() {
        return Err(portal_failure(
            "network",
            format!("The portal server failed (HTTP {}).", status.as_u16()),
            &detail,
        ));
    }
    // The create response carries no secret, so the new key is read back from the list
    // and revealed like an existing one.
    let keys = list_keys(&client, &cookie_header, &secrets).await?;
    let key = find_reusable_key(&keys, &name).ok_or_else(|| {
        portal_failure(
            "parse",
            "The portal did not list the key it just created.",
            &detail,
        )
    })?;
    let id = key.id;
    let token = reveal_secret(
        &client,
        &cookie_header,
        state.csrftoken().as_deref(),
        id,
        &secrets,
    )
    .await?;
    state.set_token(&token);
    Ok(CreatedKey { name, token })
}

#[tauri::command]
pub async fn portal_close<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, PortalState>,
) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = window.destroy();
    }
    state.clear();
    Ok(())
}

fn portal_client(redirect: reqwest::redirect::Policy) -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(concat!(
            "epfl-openwork-installer/",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(redirect)
        // The portal is internal-only and can hang: bound every request so the
        // UI keeps control and can surface a typed network error.
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| {
            AppError::new(
                "network",
                format!("Could not create the HTTP client: {error}"),
            )
        })
}

struct FetchedPage {
    status: u16,
    final_url: String,
    body: String,
    set_cookies: Vec<String>,
}

impl FetchedPage {
    fn detail(&self, secrets: &[String]) -> String {
        endpoint_detail(self.status, &self.final_url, &self.body, secrets)
    }
}

/// Error whose message names the HTTP status and a masked page excerpt, so a trace
/// line explains the failure without a debugger.
fn portal_failure(kind: &str, message: impl Into<String>, detail: &str) -> AppError {
    let message = format!("{} The portal answered {detail}.", message.into());
    AppError::new(kind, message)
}

async fn fetch_page(
    client: &reqwest::Client,
    url: &str,
    cookie_header: &str,
    referer: Option<&str>,
    secrets: &[String],
) -> Result<FetchedPage, AppError> {
    let mut request = client
        .get(url)
        .header(reqwest::header::COOKIE, cookie_header);
    if let Some(referer) = referer {
        request = request.header(reqwest::header::REFERER, referer);
    }
    let response = request.send().await.map_err(|error| {
        AppError::new("network", format!("Could not reach the portal: {error}"))
    })?;
    let status = response.status();
    let final_url = response.url().clone();
    if !is_portal_page(&final_url) {
        return Err(portal_failure(
            "portal-auth",
            "The portal redirected to a sign-in page. Sign in again.",
            &endpoint_detail(status.as_u16(), final_url.as_str(), "", secrets),
        ));
    }
    let set_cookies: Vec<String> = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_string))
        .collect();
    let body = response.text().await.map_err(|error| {
        AppError::new(
            "network",
            format!("Could not read the portal response: {error}"),
        )
    })?;
    let page = FetchedPage {
        status: status.as_u16(),
        final_url: final_url.to_string(),
        body,
        set_cookies,
    };
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(portal_failure(
            "portal-auth",
            "The portal session is no longer valid. Sign in again.",
            &page.detail(secrets),
        ));
    }
    if status.is_server_error() {
        return Err(portal_failure(
            "network",
            format!("The portal server failed (HTTP {}).", status.as_u16()),
            &page.detail(secrets),
        ));
    }
    Ok(page)
}

struct FormFetch {
    units: Vec<PortalUnit>,
    existing_open_work_key: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct PortalKey {
    pub id: i64,
    pub name: String,
    /// The portal pool of the key: `BASIC` or `PREMIUM`. The key list API carries it; a
    /// pool-less answer still parses.
    #[serde(default)]
    pub pool: String,
    #[serde(default)]
    pub is_application: bool,
    #[serde(default)]
    pub is_legacy: bool,
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default)]
    pub is_pending: bool,
}

pub(crate) fn parse_keys(json: &str) -> Result<Vec<PortalKey>, AppError> {
    serde_json::from_str(json).map_err(|error| {
        AppError::new(
            "parse",
            format!("The portal key list is not the expected JSON: {error}"),
        )
    })
}

/// The personal, enabled key OpenWork can reuse under this name. Application keys need
/// a head-of-unit validation and legacy keys are deprecated, so neither is reused.
pub(crate) fn find_reusable_key<'a>(keys: &'a [PortalKey], name: &str) -> Option<&'a PortalKey> {
    keys.iter()
        .find(|key| key.name == name && is_selectable(key))
}

/// A key the user may pick for reuse. Application keys need a head-of-unit validation,
/// legacy keys are deprecated, and a disabled or pending key has no usable secret.
pub(crate) fn is_selectable(key: &PortalKey) -> bool {
    !key.is_application && !key.is_legacy && key.is_enabled && !key.is_pending
}

/// The user's own keys, reduced to what the key card shows: no token, no key material.
pub(crate) fn selectable_key_choices(keys: &[PortalKey]) -> Vec<PortalKeyChoice> {
    keys.iter()
        .filter(|key| is_selectable(key))
        .map(|key| PortalKeyChoice {
            id: key.id,
            name: key.name.clone(),
            pool: key.pool.clone(),
        })
        .collect()
}

async fn list_keys(
    client: &reqwest::Client,
    cookie_header: &str,
    secrets: &[String],
) -> Result<Vec<PortalKey>, AppError> {
    let page = fetch_page(
        client,
        &format!("{PORTAL_BASE}{KEYS_API_PATH}"),
        cookie_header,
        None,
        secrets,
    )
    .await?;
    parse_keys(&page.body)
        .map_err(|error| portal_failure(&error.kind, error.message, &page.detail(secrets)))
}

/// Asks the portal for the stored secret of one key. The keys page reveals a secret with
/// this htmx POST, so the request replays the htmx headers the page uses.
async fn reveal_secret(
    client: &reqwest::Client,
    cookie_header: &str,
    csrftoken: Option<&str>,
    id: i64,
    secrets: &[String],
) -> Result<String, AppError> {
    let mut request = client
        .post(format!("{PORTAL_BASE}{SHOW_SECRET_PATH}"))
        .header(reqwest::header::COOKIE, cookie_header)
        .header(
            reqwest::header::REFERER,
            format!("{PORTAL_BASE}{KEYS_PATH}"),
        )
        .header("HX-Request", "true")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(format!("key={id}"));
    if let Some(csrftoken) = csrftoken {
        request = request.header("X-CSRFToken", csrftoken);
    }
    let response = request.send().await.map_err(|error| {
        AppError::new(
            "network",
            format!("The portal key reveal request failed: {error}"),
        )
    })?;
    let status = response.status();
    let final_url = response.url().clone();
    let body = response.text().await.map_err(|error| {
        AppError::new(
            "network",
            format!("Could not read the key reveal response: {error}"),
        )
    })?;
    let page = FetchedPage {
        status: status.as_u16(),
        final_url: final_url.to_string(),
        body,
        set_cookies: Vec::new(),
    };
    extract_secret(&page.body).ok_or_else(|| {
        portal_failure(
            "parse",
            "The portal did not reveal the existing API key.",
            &page.detail(secrets),
        )
    })
}

async fn load_portal_form(state: &PortalState) -> Result<FormFetch, AppError> {
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new(
            "portal-auth",
            "Sign in to the portal before loading the key form.",
        )
    })?;
    let secrets = state.secrets();
    let client = portal_client(reqwest::redirect::Policy::default())?;
    let keys = list_keys(&client, &cookie_header, &secrets).await?;
    let page = fetch_page(
        &client,
        &format!("{PORTAL_BASE}{NEW_KEY_MODAL_PATH}"),
        &cookie_header,
        Some(&format!("{PORTAL_BASE}{KEYS_PATH}")),
        &secrets,
    )
    .await?;
    let parsed = parse_form(&page.body);
    // htmx sends the csrftoken cookie value in the X-CSRFToken header, so that cookie is
    // the CSRF token when the modal carries no hidden field. Django rotates it on the
    // form response, so a fresh Set-Cookie wins over the webview's value.
    let csrftoken =
        extract_cookie_value(&page.set_cookies, "csrftoken").or_else(|| state.csrftoken());
    let csrf = parsed
        .csrf
        .clone()
        .or_else(|| csrftoken.clone())
        .ok_or_else(|| {
            portal_failure(
                "parse",
                "The portal key form carries no CSRF token.",
                &page.detail(&secrets),
            )
        })?;
    let form_url = parsed
        .action
        .clone()
        .unwrap_or_else(|| page.final_url.clone());
    state.set_form(&csrf, csrftoken.as_deref(), &form_url, &parsed.units);
    Ok(FormFetch {
        units: parsed.units,
        existing_open_work_key: find_reusable_key(&keys, OPEN_WORK_KEY_NAME).is_some(),
    })
}

/// One option of the portal Unit select. The page posts the numeric id in `value` and
/// shows the unit name as the option text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PortalUnit {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Default)]
pub(crate) struct ParsedForm {
    pub csrf: Option<String>,
    pub units: Vec<PortalUnit>,
    /// Where the modal form posts, if the portal states it.
    pub action: Option<String>,
}

pub(crate) fn extract_sessionid(cookies: &[String]) -> Option<String> {
    extract_cookie_value(cookies, "sessionid")
}

pub(crate) fn extract_cookie_value(cookies: &[String], name: &str) -> Option<String> {
    cookies.iter().find_map(|cookie| {
        cookie
            .split(';')
            .next()?
            .split_once('=')
            .and_then(|(cookie_name, value)| {
                cookie_name
                    .trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
    })
}

pub(crate) fn parse_form(html: &str) -> ParsedForm {
    let document = scraper::Html::parse_document(html);
    ParsedForm {
        csrf: csrf_from_document(&document).or_else(|| csrf_from_raw_html(html)),
        units: units_from_document(&document),
        action: form_action(&document),
    }
}

/// Where the key form posts. The portal renders its forms as htmx modals, so the target
/// can sit in `action` or in an `hx-post` attribute.
fn form_action(document: &scraper::Html) -> Option<String> {
    for element in select_all(document, "form") {
        for attribute in ["hx-post", "action"] {
            if let Some(value) = element
                .value()
                .attr(attribute)
                .map(str::trim)
                .filter(|value| !value.is_empty() && !value.starts_with('#'))
            {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn csrf_from_document(document: &scraper::Html) -> Option<String> {
    for (selector, attribute) in [
        ("input[name=csrfmiddlewaretoken]", "value"),
        ("meta[name=csrf-token]", "content"),
        ("meta[name=csrfmiddlewaretoken]", "content"),
    ] {
        for element in select_all(document, selector) {
            if let Some(value) = element
                .value()
                .attr(attribute)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn csrf_from_raw_html(html: &str) -> Option<String> {
    let marker = html.find("csrfmiddlewaretoken")?;
    let after = &html[marker..html.len().min(marker + 512)];
    for quote in ['"', '\''] {
        let pattern = format!("value={quote}");
        if let Some(start) = after.find(&pattern) {
            let rest = &after[start + pattern.len()..];
            if let Some(end) = rest.find(quote) {
                let value = rest[..end].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn units_from_document(document: &scraper::Html) -> Vec<PortalUnit> {
    for selector in [
        "select[name=unit] option",
        "select[id*=unit] option",
        "select[name*=unit] option",
    ] {
        let units: Vec<PortalUnit> = select_all(document, selector)
            .iter()
            .filter_map(option_label)
            .collect();
        if !units.is_empty() {
            return units;
        }
    }
    Vec::new()
}

fn option_label(element: &scraper::ElementRef<'_>) -> Option<PortalUnit> {
    let text = element.text().collect::<String>().trim().to_string();
    if text.is_empty() || text == "---------" {
        return None;
    }
    let value = element
        .value()
        .attr("value")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&text);
    Some(PortalUnit {
        value: value.to_string(),
        label: text,
    })
}

pub(crate) fn build_create_body(csrf: &str, name: &str, unit: &str) -> String {
    form_urlencoded::Serializer::new(String::new())
        .append_pair("csrfmiddlewaretoken", csrf)
        .append_pair("name", name)
        .append_pair("unit", unit)
        .append_pair("pool", "BASIC")
        .append_pair("usage", "PERSONAL")
        .finish()
}

/// The full secret from a key reveal fragment. The portal renders it as page text (the
/// list page shows `sk-...AX3Q`), so the longest RCP key run in the text is the secret.
pub(crate) fn extract_secret(body: &str) -> Option<String> {
    let document = scraper::Html::parse_document(body);
    let text = document.root_element().text().collect::<String>();
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
        .filter(|run| run.starts_with("sk-") && run.len() > 8)
        .max_by_key(|run| run.len())
        .map(str::to_string)
}

pub(crate) fn extract_error_message(html: &str) -> Option<String> {
    let document = scraper::Html::parse_document(html);
    for selector in [
        "ul.errorlist li",
        ".errorlist li",
        "[role=alert]",
        // The key modal renders one error slot per field, named <field>_error.
        "[id$=_error]",
        "[class*=error] li",
        "[class*=error]",
    ] {
        for element in select_all(&document, selector) {
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn select_all<'a>(document: &'a scraper::Html, selector: &'a str) -> Vec<scraper::ElementRef<'a>> {
    scraper::Selector::parse(selector)
        .map(|parsed| document.select(&parsed).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    const MODAL: &str = include_str!("../fixtures/portal/keys-modal.html");
    const KEYS_API: &str = include_str!("../fixtures/portal/keys-api.json");
    const KEYS_API_DISABLED: &str = include_str!("../fixtures/portal/keys-api-disabled.json");
    const REFUSED: &str = include_str!("../fixtures/portal/keys-refused.html");
    const SECRET: &str = include_str!("../fixtures/portal/key-secret.html");
    const LOGIN_PAGE: &str = include_str!("../fixtures/portal/login-page.html");

    #[test]
    fn sessionid_extraction_from_cookie_strings() {
        let cookies = vec![
            "csrftoken=AbCdEf123456; Path=/; SameSite=Lax".to_string(),
            "sessionid=7xk2m9q0w1e3r5t7; Path=/; HttpOnly; SameSite=Lax".to_string(),
        ];
        assert_eq!(
            extract_sessionid(&cookies).as_deref(),
            Some("7xk2m9q0w1e3r5t7")
        );

        let spaced = vec!["SESSIONID = spaced.value; Path=/".to_string()];
        assert_eq!(extract_sessionid(&spaced).as_deref(), Some("spaced.value"));

        assert_eq!(
            extract_sessionid(&["csrftoken=x; Path=/".to_string()]),
            None
        );
        assert!(extract_sessionid(&[]).is_none());
    }

    #[test]
    fn csrftoken_cookie_is_captured_and_replayed() {
        let cookies = vec![
            "csrftoken=csrfCookie789; Path=/; SameSite=Lax".to_string(),
            "sessionid=sid42; Path=/; HttpOnly".to_string(),
        ];
        assert_eq!(extract_sessionid(&cookies).as_deref(), Some("sid42"));
        assert_eq!(
            extract_cookie_value(&cookies, "csrftoken").as_deref(),
            Some("csrfCookie789")
        );

        let state = PortalState::default();
        state.set_session("sid42", Some("csrfCookie789"));
        assert_eq!(
            state.cookie_header().as_deref(),
            Some("sessionid=sid42; csrftoken=csrfCookie789")
        );

        let no_csrf = PortalState::default();
        no_csrf.set_session("sid42", None);
        assert_eq!(no_csrf.cookie_header().as_deref(), Some("sessionid=sid42"));
        assert_eq!(PortalState::default().cookie_header(), None);
    }

    #[test]
    fn anonymous_sessionid_is_not_a_finished_login() {
        // Live check on 2026-10-30: GET /aiaas/keys answers 302 /accounts/login -> 302
        // /accounts/oidc/authenticate/ and that hop already sets an anonymous
        // sessionid. A cookie in the window therefore proves nothing.
        let mut tracker = LoginTracker::default();
        assert_eq!(tracker.on_cookies(None), PollAction::Wait);
        assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Verify);
        // A rejected probe must not fire a request on every tick...
        for _ in 0..SESSION_REPROBE_TICKS {
            assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Wait);
        }
        // ...but it is retried, in case the portal keeps the session key across login.
        assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Verify);
        for _ in 0..SESSION_REPROBE_TICKS {
            assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Wait);
        }
        assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Verify);
    }

    #[test]
    fn rotated_sessionid_is_verified_without_delay() {
        let mut tracker = LoginTracker::default();
        assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Verify);
        assert_eq!(tracker.on_cookies(Some("anon-session")), PollAction::Wait);
        assert_eq!(
            tracker.on_cookies(Some("logged-in-session")),
            PollAction::Verify
        );
    }

    #[test]
    fn session_cookie_header_pairs_sessionid_and_csrftoken() {
        assert_eq!(
            session_cookie_header("sid42", Some("csrfCookie789")),
            "sessionid=sid42; csrftoken=csrfCookie789"
        );
        assert_eq!(session_cookie_header("sid42", None), "sessionid=sid42");
    }

    #[test]
    fn form_parse_from_the_live_modal() {
        let parsed = parse_form(MODAL);
        assert_eq!(parsed.csrf.as_deref(), Some("***"));
        assert_eq!(
            parsed.units,
            vec![
                PortalUnit {
                    value: "10208".to_string(),
                    label: "ENAC-IT".to_string(),
                },
                PortalUnit {
                    value: "10232".to_string(),
                    label: "LSMS".to_string(),
                },
            ]
        );
        // The modal form posts through its htmx button, not through a form action.
        assert_eq!(parsed.action, None);
    }

    #[test]
    fn form_parse_on_a_page_without_a_form_reports_nothing() {
        let parsed = parse_form(LOGIN_PAGE);
        assert_eq!(parsed.csrf, None);
        assert!(parsed.units.is_empty());
        assert_eq!(parsed.action, None);
    }

    #[test]
    fn create_body_matches_the_live_modal_values() {
        assert_eq!(
            build_create_body("abc123", "OpenWork", "10208"),
            "csrfmiddlewaretoken=abc123&name=OpenWork&unit=10208&pool=BASIC&usage=PERSONAL"
        );
        assert_eq!(
            build_create_body("a&b=c", "OpenWork Two", "10208"),
            "csrfmiddlewaretoken=a%26b%3Dc&name=OpenWork+Two&unit=10208&pool=BASIC&usage=PERSONAL"
        );
    }

    #[test]
    fn secret_extraction_from_the_reveal_fragment() {
        assert_eq!(
            extract_secret(SECRET).as_deref(),
            Some("sk-fixture-not-a-real-key")
        );
    }

    #[test]
    fn secret_extraction_ignores_the_masked_key_name() {
        let list_row = r#"<td>OpenWork</td><td>sk-...AX3Q</td>"#;
        assert_eq!(extract_secret(list_row), None);
    }

    #[test]
    fn reusable_key_is_the_personal_enabled_one() {
        let keys = parse_keys(KEYS_API).expect("key list parses");
        let key = find_reusable_key(&keys, "OpenWork").expect("OpenWork key is reusable");
        assert_eq!(key.id, 1462);
        assert!(find_reusable_key(&keys, "Other-Key").is_none());
        let disabled = parse_keys(KEYS_API_DISABLED).unwrap();
        assert!(find_reusable_key(&disabled, "OpenWork").is_none());
    }

    #[test]
    fn key_list_carries_the_pool_and_tolerates_a_missing_one() {
        let keys = parse_keys(KEYS_API).expect("key list parses");
        assert_eq!(keys[0].pool, "BASIC");
        let sparse = parse_keys(r#"[{"id": 7, "name": "Bare"}]"#).expect("a pool-less key parses");
        assert_eq!(sparse[0].pool, "");
    }

    #[test]
    fn selectable_keys_skip_application_legacy_disabled_and_pending() {
        let keys = parse_keys(KEYS_API).expect("key list parses");
        assert_eq!(
            selectable_key_choices(&keys),
            vec![PortalKeyChoice {
                id: 1462,
                name: "OpenWork".to_string(),
                pool: "BASIC".to_string(),
            }]
        );

        let extra = parse_keys(
            r#"[
            {"id": 1, "name": "Legacy", "is_enabled": true, "is_legacy": true},
            {"id": 2, "name": "Pending", "is_enabled": true, "is_pending": true},
            {"id": 3, "name": "Premium", "is_enabled": true, "pool": "PREMIUM"}
        ]"#,
        )
        .expect("key list parses");
        assert_eq!(
            selectable_key_choices(&extra),
            vec![PortalKeyChoice {
                id: 3,
                name: "Premium".to_string(),
                pool: "PREMIUM".to_string(),
            }]
        );

        let disabled = parse_keys(KEYS_API_DISABLED).expect("key list parses");
        assert!(selectable_key_choices(&disabled).is_empty());
    }

    #[test]
    fn key_list_parse_rejects_html() {
        assert_eq!(parse_keys(MODAL).unwrap_err().kind, "parse");
    }

    #[test]
    fn portal_refusal_message_extraction() {
        let msg = extract_error_message(REFUSED).expect("message found");
        assert!(msg.contains("already exists"));
        assert_eq!(extract_error_message(MODAL), None);
        assert_eq!(extract_error_message(SECRET), None);
    }

    #[test]
    fn portal_page_check_needs_host_and_aiaas_path() {
        let ok: reqwest::Url = "https://portal.rcp.epfl.ch/aiaas/keys".parse().unwrap();
        let wrong_host: reqwest::Url = "https://evil.example.com/aiaas/keys".parse().unwrap();
        let wrong_path: reqwest::Url = "https://portal.rcp.epfl.ch/accounts/login".parse().unwrap();
        assert!(is_portal_page(&ok));
        assert!(!is_portal_page(&wrong_host));
        assert!(!is_portal_page(&wrong_path));
    }

    const SECRET_SESSION: &str = "sid-9f8e7d6c5b4a3210";
    const SECRET_TOKEN: &str = "rk-live-0123456789abcdef0123456789";

    #[test]
    fn page_excerpt_shows_readable_text_without_scripts_or_secrets() {
        let body = format!(
            r#"<html><head><title>API keys</title>
            <script>var hidden = "inline-script-must-not-show";</script>
            <style>.x {{ color: red; }}</style></head>
            <body><h1>API keys</h1><p>No key yet.</p>
            <p>session {SECRET_SESSION}</p><code>{SECRET_TOKEN}</code></body></html>"#
        );

        let excerpt = page_excerpt(&body, &[SECRET_SESSION.to_string()]);

        assert!(excerpt.contains("API keys"), "excerpt: {excerpt}");
        assert!(excerpt.contains("No key yet."), "excerpt: {excerpt}");
        assert!(
            !excerpt.contains("inline-script-must-not-show"),
            "script leaked: {excerpt}"
        );
        assert!(!excerpt.contains("color: red"), "style leaked: {excerpt}");
        assert!(
            !excerpt.contains(SECRET_SESSION),
            "session leaked: {excerpt}"
        );
        assert!(!excerpt.contains(SECRET_TOKEN), "token leaked: {excerpt}");
    }

    #[test]
    fn page_excerpt_stays_short() {
        let body = format!("<body><p>{}</p></body>", "portal text ".repeat(100));

        let excerpt = page_excerpt(&body, &[]);

        assert!(excerpt.ends_with("..."), "excerpt: {excerpt}");
        assert_eq!(excerpt.chars().count(), EXCERPT_LIMIT + 3);
    }

    #[test]
    fn endpoint_detail_names_the_status_and_the_path() {
        assert_eq!(
            endpoint_detail(
                403,
                "https://portal.rcp.epfl.ch/aiaas/keys/new?next=1",
                "<body><p>CSRF verification failed.</p></body>",
                &[]
            ),
            r#"HTTP 403 on /aiaas/keys/new: "CSRF verification failed.""#
        );
        assert_eq!(
            endpoint_detail(500, "https://portal.rcp.epfl.ch/aiaas/keys", "", &[]),
            "HTTP 500 on /aiaas/keys"
        );
    }

    #[test]
    fn get_form_without_session_is_portal_auth() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let err = tauri::async_runtime::block_on(portal_get_form(app.state::<PortalState>()))
            .unwrap_err();
        assert_eq!(err.kind, "portal-auth");
    }

    #[test]
    fn create_key_without_session_is_portal_auth() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let err = tauri::async_runtime::block_on(portal_create_key(
            "OpenWork".into(),
            Some("ENAC-IT".into()),
            app.state::<PortalState>(),
        ))
        .unwrap_err();
        assert_eq!(err.kind, "portal-auth");
    }

    #[test]
    fn list_keys_without_session_is_portal_auth() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let err = tauri::async_runtime::block_on(portal_list_keys(app.state::<PortalState>()))
            .unwrap_err();
        assert_eq!(err.kind, "portal-auth");
    }

    #[test]
    fn use_key_without_session_is_portal_auth() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let err = tauri::async_runtime::block_on(portal_use_key(1462, app.state::<PortalState>()))
            .unwrap_err();
        assert_eq!(err.kind, "portal-auth");
    }

    #[test]
    fn close_drops_all_session_state() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let state = app.state::<PortalState>();
        state.set_session("sid-123", None);
        state.set_form(
            "csrf-456",
            Some("csrfCookie789"),
            "https://portal.rcp.epfl.ch/aiaas/modal_key_new",
            &[PortalUnit {
                value: "10208".to_string(),
                label: "ENAC-IT".to_string(),
            }],
        );
        state.set_token("tok-789");

        tauri::async_runtime::block_on(portal_close(
            app.handle().clone(),
            app.state::<PortalState>(),
        ))
        .expect("close succeeds");

        let state = app.state::<PortalState>();
        assert_eq!(state.cookie_header(), None);
        assert_eq!(state.csrf(), None);
        assert_eq!(state.token(), None);
        assert_eq!(state.form_url(), None);
    }
}
