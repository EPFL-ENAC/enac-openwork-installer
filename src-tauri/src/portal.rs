use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::types::{AppError, CreatedKey, PortalForm};

pub(crate) const PORTAL_BASE: &str = "https://portal.rcp.epfl.ch";
pub(crate) const KEYS_PATH: &str = "/aiaas/keys";
// Assumed form path: the flow prefers the create-link found on the keys overview and only
// falls back to this constant. Verify against the live portal in the parent live test (#1).
pub(crate) const FORM_PATH: &str = "/aiaas/keys/new";
const LOGIN_WINDOW_LABEL: &str = "portal-login";
const LOGIN_TIMEOUT_SECS: u64 = 600;
const COOKIE_POLL_MS: u64 = 1000;
const COOKIE_POLL_MAX_ERRORS: u32 = 15;
const PORTAL_HOST: &str = "portal.rcp.epfl.ch";

fn is_portal_page(url: &reqwest::Url) -> bool {
    url.host_str() == Some(PORTAL_HOST) && url.path().starts_with("/aiaas")
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
    units: Vec<String>,
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

    fn units(&self) -> Vec<String> {
        self.lock().units.clone()
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

    // Django CSRF middleware compares the form token against the csrftoken cookie
    // by default, so requests must replay both cookies captured in the webview.
    fn cookie_header(&self) -> Option<String> {
        let session = self.lock();
        let sessionid = session.sessionid.as_deref()?;
        Some(match session.csrftoken.as_deref() {
            Some(csrftoken) => format!("sessionid={sessionid}; csrftoken={csrftoken}"),
            None => format!("sessionid={sessionid}"),
        })
    }

    fn set_form(&self, csrf: &str, csrftoken: Option<&str>, form_url: &str, units: &[String]) {
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
            if let Some(sessionid) = extract_sessionid(&cookie_strings) {
                if login_state.login_generation() == generation {
                    login_state.set_session(
                        &sessionid,
                        extract_cookie_value(&cookie_strings, "csrftoken").as_deref(),
                    );
                    emit_login_result(&poll_app, true, None);
                    let _ = window.close();
                }
                return;
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn portal_get_form(state: State<'_, PortalState>) -> Result<PortalForm, AppError> {
    let form = load_portal_form(state.inner()).await?;
    Ok(PortalForm {
        units: form.units,
        existing_open_work_key: form.existing_open_work_key,
    })
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
    let csrf = state
        .csrf()
        .ok_or_else(|| AppError::new("parse", "The key creation form did not load."))?;
    let form_url = state
        .form_url()
        .ok_or_else(|| AppError::new("parse", "The key creation form did not load."))?;
    let unit =
        match unit
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(picked) => picked.to_string(),
            None => state.units().first().cloned().ok_or_else(|| {
                AppError::new("parse", "The portal form offers no unit for the key.")
            })?,
        };

    let client = portal_client()?;
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new(
            "portal-auth",
            "Sign in to the portal before creating a key.",
        )
    })?;
    let response = client
        .post(&form_url)
        .header(reqwest::header::COOKIE, cookie_header)
        .header(reqwest::header::REFERER, &form_url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(build_create_body(&csrf, &name, &unit))
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

    if !is_portal_page(&final_url) {
        return Err(AppError::new(
            "portal-auth",
            "The portal ended the session during key creation. Sign in again.",
        ));
    }
    // Token first: a success page may carry unrelated error-styled elements, and the
    // token is the only signal that the key was really created.
    if let Some(token) = extract_token(&body) {
        state.set_token(&token);
        return Ok(CreatedKey { name, token });
    }
    if let Some(message) = extract_error_message(&body) {
        return Err(AppError::new("portal-refused", message));
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::new(
            "portal-auth",
            "The portal rejected the key creation. The session or CSRF token expired, so load the form and try again.",
        ));
    }
    if status.is_server_error() {
        return Err(AppError::new(
            "network",
            format!("The portal server failed (HTTP {}).", status.as_u16()),
        ));
    }
    Err(AppError::new(
        "parse",
        "The portal did not reveal the new API token. Check the portal in a browser.",
    ))
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

fn portal_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(concat!("epfl-openwork-installer/", env!("CARGO_PKG_VERSION")))
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
    final_url: String,
    body: String,
    set_cookies: Vec<String>,
}

async fn fetch_page(
    client: &reqwest::Client,
    url: &str,
    cookie_header: &str,
    referer: Option<&str>,
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
        return Err(AppError::new(
            "portal-auth",
            "The portal redirected to a sign-in page. Sign in again.",
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
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::new(
            "portal-auth",
            "The portal session is no longer valid. Sign in again.",
        ));
    }
    if status.is_server_error() {
        return Err(AppError::new(
            "network",
            format!("The portal server failed (HTTP {}).", status.as_u16()),
        ));
    }
    Ok(FetchedPage {
        final_url: final_url.to_string(),
        body,
        set_cookies,
    })
}

struct FormFetch {
    units: Vec<String>,
    existing_open_work_key: bool,
}

async fn load_portal_form(state: &PortalState) -> Result<FormFetch, AppError> {
    let cookie_header = state.cookie_header().ok_or_else(|| {
        AppError::new(
            "portal-auth",
            "Sign in to the portal before loading the key form.",
        )
    })?;
    let client = portal_client()?;
    let overview = fetch_page(
        &client,
        &format!("{PORTAL_BASE}{KEYS_PATH}"),
        &cookie_header,
        None,
    )
    .await?;
    let existing_open_work_key = detect_existing_key(&overview.body);
    // Prefer the page linked from the overview; an inline form on the overview and the
    // constant fallback are only used when the overview carries no create link.
    let (csrf, form_url, units, set_cookies) = match find_form_url(&overview.body, PORTAL_BASE) {
        Some(form_url) => {
            let page = fetch_page(
                &client,
                &form_url,
                &cookie_header,
                Some(&overview.final_url),
            )
            .await?;
            let parsed = parse_form(&page.body)?;
            (parsed.csrf, page.final_url, parsed.units, page.set_cookies)
        }
        None => match parse_form(&overview.body) {
            Ok(parsed) => (
                parsed.csrf,
                overview.final_url.clone(),
                parsed.units,
                overview.set_cookies.clone(),
            ),
            Err(_) => {
                let form_url = format!("{PORTAL_BASE}{FORM_PATH}");
                let page = fetch_page(
                    &client,
                    &form_url,
                    &cookie_header,
                    Some(&overview.final_url),
                )
                .await?;
                let parsed = parse_form(&page.body)?;
                (parsed.csrf, page.final_url, parsed.units, page.set_cookies)
            }
        },
    };
    // Django pairs the parsed csrfmiddlewaretoken with a csrftoken cookie. When our GET
    // had no csrftoken cookie, the form response sets a fresh one: use it over the
    // webview's value, otherwise the POST would fail the CSRF check.
    let csrftoken = extract_cookie_value(&set_cookies, "csrftoken");
    state.set_form(&csrf, csrftoken.as_deref(), &form_url, &units);
    Ok(FormFetch {
        units,
        existing_open_work_key,
    })
}

#[derive(Debug)]
pub(crate) struct ParsedForm {
    pub csrf: String,
    pub units: Vec<String>,
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

pub(crate) fn parse_form(html: &str) -> Result<ParsedForm, AppError> {
    let document = scraper::Html::parse_document(html);
    let csrf = csrf_from_document(&document)
        .or_else(|| csrf_from_raw_html(html))
        .ok_or_else(|| {
            AppError::new(
                "parse",
                "Could not find the CSRF token in the portal key form.",
            )
        })?;
    let units = units_from_document(&document);
    if units.is_empty() {
        return Err(AppError::new(
            "parse",
            "Could not find the Unit options in the portal key form.",
        ));
    }
    Ok(ParsedForm { csrf, units })
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

fn units_from_document(document: &scraper::Html) -> Vec<String> {
    for selector in [
        "select[name=unit] option",
        "select[id*=unit] option",
        "select[name*=unit] option",
    ] {
        let units: Vec<String> = select_all(document, selector)
            .iter()
            .filter_map(|element| option_label(element))
            .collect();
        if !units.is_empty() {
            return units;
        }
    }
    Vec::new()
}

fn option_label(element: &scraper::ElementRef<'_>) -> Option<String> {
    let text = element.text().collect::<String>().trim().to_string();
    if let Some(value) = element
        .value()
        .attr("value")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return if text == "---------" {
            None
        } else {
            Some(value.to_string())
        };
    }
    if text.is_empty() || text == "---------" {
        return None;
    }
    Some(text)
}

pub(crate) fn build_create_body(csrf: &str, name: &str, unit: &str) -> String {
    form_urlencoded::Serializer::new(String::new())
        .append_pair("csrfmiddlewaretoken", csrf)
        .append_pair("name", name)
        .append_pair("unit", unit)
        .append_pair("pool", "Basic")
        .append_pair("usage", "Personal")
        .finish()
}

pub(crate) fn extract_token(body: &str) -> Option<String> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(token) = json_token(&value) {
                return Some(token);
            }
        }
    }
    let document = scraper::Html::parse_document(body);
    for selector in [
        "input[id*=token]",
        "input[name*=token]",
        "input[class*=token]",
        "textarea[id*=token]",
        "textarea[name*=token]",
        "textarea[class*=token]",
    ] {
        for element in select_all(&document, selector) {
            if element.value().attr("name") == Some("csrfmiddlewaretoken") {
                continue;
            }
            if let Some(found) = element
                .value()
                .attr("value")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(found.to_string());
            }
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    for selector in ["pre code", "code", "pre"] {
        for element in select_all(&document, selector) {
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn json_token(value: &serde_json::Value) -> Option<String> {
    let object = value.as_object()?;
    for key in ["token", "api_key", "apiKey", "key"] {
        if let Some(found) = object
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(found.to_string());
        }
    }
    None
}

pub(crate) fn detect_existing_key(html: &str) -> bool {
    let document = scraper::Html::parse_document(html);
    for selector in ["td", "li", "[class*=name]", "[data-name]"] {
        for element in select_all(&document, selector) {
            if element.text().collect::<String>().trim() == "OpenWork" {
                return true;
            }
        }
    }
    html.contains(">OpenWork<")
}

pub(crate) fn extract_error_message(html: &str) -> Option<String> {
    let document = scraper::Html::parse_document(html);
    for selector in [
        "ul.errorlist li",
        ".errorlist li",
        "[role=alert]",
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

pub(crate) fn find_form_url(html: &str, base: &str) -> Option<String> {
    let base_url = tauri::Url::parse(base).ok()?;
    let document = scraper::Html::parse_document(html);
    for element in select_all(&document, "a[href]") {
        let href = element.value().attr("href").unwrap_or_default().trim();
        if href.is_empty() {
            continue;
        }
        let label = element.text().collect::<String>().trim().to_lowercase();
        let path = href
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let last_segment = path
            .rsplit('/')
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        let looks_like_create = label.contains("create")
            || label.contains("new key")
            || last_segment.starts_with("new")
            || last_segment.starts_with("create")
            || last_segment.starts_with("add");
        if !looks_like_create {
            continue;
        }
        // The key form lives inside the /aiaas app: a "Create account" nav link
        // under /accounts must not be mistaken for the create-form link, or the
        // fetch would fail as an expired session instead of falling back.
        let Ok(joined) = base_url.join(href) else {
            continue;
        };
        if joined.host_str() == base_url.host_str() && joined.path().starts_with("/aiaas") {
            return Some(joined.to_string());
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

    const OVERVIEW: &str = include_str!("../fixtures/portal/keys-overview.html");
    const OVERVIEW_PROSE: &str =
        include_str!("../fixtures/portal/keys-overview-openwork-prose.html");
    const FORM: &str = include_str!("../fixtures/portal/keys-form.html");
    const FORM_ALT: &str = include_str!("../fixtures/portal/keys-form-alt.html");
    const REFUSED: &str = include_str!("../fixtures/portal/keys-refused.html");
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
    fn form_parse_from_django_markup() {
        let parsed = parse_form(FORM).expect("form parses");
        assert_eq!(parsed.csrf, "formCsrfToken456defGHI789");
        assert_eq!(parsed.units, vec!["ENAC-IT", "FSV-LBO", "CIGALE"]);
    }

    #[test]
    fn form_parse_tolerates_alternate_markup() {
        let parsed = parse_form(FORM_ALT).expect("alt form parses");
        assert_eq!(parsed.csrf, "altMarkupCsrf000aaa111bbb");
        assert_eq!(parsed.units, vec!["ENAC", "FSV", "SB"]);
    }

    #[test]
    fn form_parse_fails_typed_on_pages_without_a_form() {
        let err = parse_form(LOGIN_PAGE).unwrap_err();
        assert_eq!(err.kind, "parse");
    }

    #[test]
    fn create_body_has_exact_fields_and_urlencoding() {
        assert_eq!(
            build_create_body("abc123", "OpenWork", "ENAC-IT"),
            "csrfmiddlewaretoken=abc123&name=OpenWork&unit=ENAC-IT&pool=Basic&usage=Personal"
        );
        assert_eq!(
            build_create_body("a&b=c", "OpenWork Two", "ENAC-IT"),
            "csrfmiddlewaretoken=a%26b%3Dc&name=OpenWork+Two&unit=ENAC-IT&pool=Basic&usage=Personal"
        );
    }

    #[test]
    fn token_extraction_input_variant() {
        assert_eq!(
            extract_token(include_str!("../fixtures/portal/key-created-input.html")).as_deref(),
            Some("rk-live-9f8e7d6c5b4a3210fedcba")
        );
    }

    #[test]
    fn token_extraction_textarea_variant() {
        assert_eq!(
            extract_token(include_str!("../fixtures/portal/key-created-textarea.html")).as_deref(),
            Some("rk-live-aabbccddeeff001122334455")
        );
    }

    #[test]
    fn token_extraction_code_variant() {
        assert_eq!(
            extract_token(include_str!("../fixtures/portal/key-created-code.html")).as_deref(),
            Some("rk-live-00112233445566778899aabb")
        );
    }

    #[test]
    fn token_extraction_json_variant() {
        assert_eq!(
            extract_token(include_str!("../fixtures/portal/key-created.json")).as_deref(),
            Some("rk-live-json0123456789abcdef")
        );
    }

    #[test]
    fn token_extraction_absent() {
        assert_eq!(extract_token(FORM), None);
        assert_eq!(extract_token(OVERVIEW), None);
    }

    #[test]
    fn existing_key_detection() {
        assert!(detect_existing_key(OVERVIEW));
        assert!(!detect_existing_key(OVERVIEW_PROSE));
        assert!(!detect_existing_key(FORM));
    }

    #[test]
    fn portal_refusal_message_extraction() {
        let msg = extract_error_message(REFUSED).expect("message found");
        assert!(msg.contains("maximum number of API keys"));
        assert_eq!(extract_error_message(OVERVIEW), None);
        assert_eq!(extract_error_message(FORM), None);
    }

    #[test]
    fn token_wins_over_error_styling_on_success_pages() {
        let mixed = r#"
            <div class="alert alert-danger" role="alert">Legacy notice, ignore.</div>
            <p>Your new token:</p>
            <input id="id_token" value="rk-live-mixed000111222333">
        "#;
        assert_eq!(
            extract_token(mixed).as_deref(),
            Some("rk-live-mixed000111222333")
        );
        assert_eq!(extract_token(REFUSED), None);
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

    #[test]
    fn form_url_discovery_from_overview_page() {
        assert_eq!(
            find_form_url(OVERVIEW, PORTAL_BASE).as_deref(),
            Some("https://portal.rcp.epfl.ch/aiaas/keys/new")
        );
        assert_eq!(find_form_url(FORM_ALT, PORTAL_BASE), None);
    }

    #[test]
    fn form_url_discovery_ignores_create_links_outside_aiaas() {
        let page = r#"
            <nav><a href="/accounts/register">Create account</a></nav>
            <a href="https://docs.example.com/create-token">Create a token</a>
        "#;
        assert_eq!(find_form_url(page, PORTAL_BASE), None);
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
    fn close_drops_all_session_state() {
        let app = tauri::test::mock_app();
        app.manage(PortalState::default());
        let state = app.state::<PortalState>();
        state.set_session("sid-123", None);
        state.set_form(
            "csrf-456",
            Some("csrfCookie789"),
            "https://portal.rcp.epfl.ch/aiaas/keys/new",
            &["ENAC-IT".into()],
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
