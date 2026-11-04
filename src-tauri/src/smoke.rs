use crate::openwork;
use crate::types::{AppError, SmokeResult};
use crate::util::{self, Logger};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PROVIDER: &str = "epfl-rcp-aiaas";
pub const MODEL: &str = "epfl-rcp-aiaas/epfl-aiaas/small";
const TIMEOUT: Duration = Duration::from_secs(60);
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(300);
const PROMPT: &str = "Reply with the single word OK.";
const DETAIL_LIMIT: usize = 500;
const OUTPUT_LINE_LIMIT: usize = 10;
const SIDECARS: &str = "sidecars";
const ENGINE_V1: &str = "opencode";
const ENGINE_V2: &str = "opencode2";
const V2_MARKER: &str = "engine-v2-preview.json";
const V2_ENV: &str = "OPENWORK_ENGINE_V2_PREVIEW";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    V1,
    V2,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Engine::V1 => ENGINE_V1,
            Engine::V2 => ENGINE_V2,
        }
    }
}

/// A fresh install of this run ships the v2 engine. An older install keeps v1
/// unless the preview marker or the environment variable opts in.
pub fn engine_for(fresh_install: bool, marker: bool, env_v2: bool) -> Engine {
    if fresh_install || marker || env_v2 {
        Engine::V2
    } else {
        Engine::V1
    }
}

/// The bundle ships the plain name and arch-suffixed variants; the plain name wins.
pub fn candidates(dir: &Path, engine: Engine, arch: &str, windows: bool) -> Vec<PathBuf> {
    let exe = if windows { ".exe" } else { "" };
    let mut names = vec![format!("{}{exe}", engine.name())];
    for suffix in arch_suffixes(arch) {
        names.push(format!("{}-{suffix}{exe}", engine.name()));
    }
    names.into_iter().map(|name| dir.join(name)).collect()
}

fn arch_suffixes(arch: &str) -> &'static [&'static str] {
    match arch {
        "aarch64" => &["aarch64", "arm64"],
        "x86_64" => &["x86_64", "x64"],
        _ => &[],
    }
}

/// Sidecars live under `resources/sidecars`: inside the app bundle on macOS,
/// inside the install directory of a bare folder, next to an executable.
pub fn sidecar_dir(install: &Path) -> PathBuf {
    let ext = install
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if ext.eq_ignore_ascii_case("app") {
        return install.join("Contents").join("Resources").join(SIDECARS);
    }
    if ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("appimage") {
        return install
            .parent()
            .unwrap_or(install)
            .join("resources")
            .join(SIDECARS);
    }
    install.join("resources").join(SIDECARS)
}

pub fn pick(dir: &Path, engine: Engine) -> Option<PathBuf> {
    candidates(dir, engine, util::arch(), cfg!(windows))
        .into_iter()
        .find(|path| path.is_file())
}

fn is_appimage(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
}

/// The AppImage keeps the sidecars inside its squashfs, so unpack them once into
/// a temp directory and run the extracted copy. The caller removes the directory.
async fn extract_sidecars(appimage: &Path) -> Option<(PathBuf, PathBuf)> {
    let dir = util::temp_dir("sidecars").ok()?;
    let pattern = format!("resources/{SIDECARS}/*");
    let output = util::run_in(
        appimage,
        &["--appimage-extract", &pattern],
        EXTRACT_TIMEOUT,
        &[],
        Some(dir.clone()),
    )
    .await
    .ok()?;
    let extracted = dir.join("squashfs-root").join("resources").join(SIDECARS);
    if output.success && extracted.is_dir() {
        return Some((dir, extracted));
    }
    util::remove_dir(&dir);
    None
}

/// Finds the engine binary and, for an AppImage, the temp directory that holds it.
async fn locate(install: &Path, engine: Engine) -> Option<(PathBuf, Option<PathBuf>)> {
    if !is_appimage(install) {
        return pick(&sidecar_dir(install), engine).map(|path| (path, None));
    }
    let (dir, sidecars) = extract_sidecars(install).await?;
    match pick(&sidecars, engine) {
        Some(path) => Some((path, Some(dir))),
        None => {
            util::remove_dir(&dir);
            None
        }
    }
}

fn v2_marker() -> bool {
    util::home_dir()
        .map(|home| home.join(".config").join("openwork").join(V2_MARKER))
        .is_some_and(|path| path.is_file())
}

fn v2_env() -> bool {
    std::env::var(V2_ENV).is_ok_and(|value| !value.is_empty() && value != "0")
}

async fn version_at(binary: &Path) -> Option<String> {
    let output = util::run(binary, &["--version"], VERSION_TIMEOUT)
        .await
        .ok()?;
    parse_version(&output.stdout).or_else(|| parse_version(&output.stderr))
}

pub fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_start_matches(['v', 'V'])
            .trim_end_matches(['.', ',', ')', ']', ';']);
        let first_digit = candidate.starts_with(|c: char| c.is_ascii_digit());
        let version_like = candidate.contains('.')
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '~'));
        (first_digit && version_like).then(|| candidate.to_string())
    })
}

fn smoke_args() -> [&'static str; 5] {
    ["run", "--standalone", "--model", MODEL, PROMPT]
}

fn strip_ansi(line: &str) -> String {
    let mut clean = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            clean.push(c);
            continue;
        }
        if chars.next() != Some('[') {
            continue;
        }
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    clean
}

fn meaningful_lines(output: &util::CommandOutput) -> Vec<String> {
    output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .map(strip_ansi)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .take(OUTPUT_LINE_LIMIT)
        .collect()
}

fn is_header(line: &str) -> bool {
    matches!(
        line.to_lowercase().as_str(),
        "error" | "usage" | "description" | "arguments" | "flags"
    )
}

/// A usage or help dump anywhere in the capture means the CLI rejected the
/// invocation shape, not the credential or the model.
fn is_usage(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let lower = line.to_lowercase();
        lower.starts_with("usage") || lower.starts_with("opencode run")
    })
}

fn headline(lines: &[String]) -> &str {
    lines
        .iter()
        .map(String::as_str)
        .find(|line| !is_header(line))
        .unwrap_or("no output")
}

fn failure_message(args: &[&str], version: &str, output: &util::CommandOutput) -> String {
    let lines = meaningful_lines(output);
    let first = headline(&lines);
    let shape = if is_usage(&lines) {
        format!("usage or help printed, args were: {}", args.join(" "))
    } else {
        "runtime error, see output below (auth, model or network)".to_string()
    };
    let code = output
        .code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let body = lines
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "the {PROVIDER} check failed ({shape}): {first}\nexit code: {code}\nopencode version: {version}\noutput:\n{body}"
    )
}

#[tauri::command]
pub async fn smoke_test(
    app: tauri::AppHandle,
    token: Option<String>,
    fresh_install: bool,
) -> Result<SmokeResult, AppError> {
    let secret = token.as_deref();
    let status = openwork::openwork_status().await?;
    let Some(install) = status.path.map(PathBuf::from) else {
        return Ok(skipped("OpenWork is not installed"));
    };

    let engine = engine_for(fresh_install, v2_marker(), v2_env());
    let Some((binary, cleanup)) = locate(&install, engine).await else {
        return Ok(skipped(&format!(
            "the OpenWork {} engine was not found",
            engine.name()
        )));
    };

    // The sidecar reads its key from the environment, never from the command
    // line. Standalone mode keeps the check off the shared OpenCode server, so a
    // stale background server cannot fail the test.
    let args = smoke_args();
    let version = version_at(&binary)
        .await
        .unwrap_or_else(|| "unknown".to_string());
    let mut envs: Vec<(&str, String)> = Vec::new();
    if let Some(token) = secret {
        envs.push((openwork::OPENWORK_ENV_KEY, token.to_string()));
    }
    let output = util::run_in(&binary, &args, TIMEOUT, &envs, None).await;
    if let Some(dir) = cleanup {
        util::remove_dir(&dir);
    }
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Ok(failed(
                format!("the {PROVIDER} check failed: {}", error.message),
                secret,
            ))
        }
    };
    if !output.success {
        let full = format!("stderr:\n{}\nstdout:\n{}", output.stderr, output.stdout);
        Logger::new(&app, "smoke").log(util::mask_secret(&full, secret));
        return Ok(failed(failure_message(&args, &version, &output), secret));
    }
    let reply = output.stdout.trim().to_string();
    if reply.is_empty() {
        return Ok(failed(format!("{MODEL} answered with no text"), secret));
    }

    Ok(SmokeResult {
        ok: true,
        skipped: false,
        detail: Some(format!(
            "{MODEL} answered: {}",
            truncate(&reply, DETAIL_LIMIT)
        )),
        error: None,
    })
}

fn skipped(reason: &str) -> SmokeResult {
    SmokeResult {
        ok: false,
        skipped: true,
        detail: Some(format!("the engine check is skipped: {reason}")),
        error: None,
    }
}

fn failed(message: impl Into<String>, secret: Option<&str>) -> SmokeResult {
    SmokeResult {
        ok: false,
        skipped: false,
        detail: None,
        error: Some(util::mask_secret(&message.into(), secret)),
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_TOKEN: &str = "sk-live-fixture-0123456789";

    #[test]
    fn a_fresh_install_runs_the_v2_engine_and_an_older_one_v1() {
        assert_eq!(engine_for(true, false, false), Engine::V2);
        assert_eq!(engine_for(false, false, false), Engine::V1);
        assert_eq!(engine_for(false, true, false), Engine::V2);
        assert_eq!(engine_for(false, false, true), Engine::V2);
    }

    #[test]
    fn the_plain_sidecar_name_comes_before_the_arch_suffixed_ones() {
        let paths = candidates(Path::new("/w/sidecars"), Engine::V2, "aarch64", false);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/w/sidecars/opencode2"),
                PathBuf::from("/w/sidecars/opencode2-aarch64"),
                PathBuf::from("/w/sidecars/opencode2-arm64"),
            ]
        );

        let exe = candidates(Path::new("C:/w"), Engine::V1, "x86_64", true);
        assert_eq!(exe[0], PathBuf::from("C:/w/opencode.exe"));
        assert_eq!(exe[1], PathBuf::from("C:/w/opencode-x86_64.exe"));
    }

    #[test]
    fn the_sidecar_dir_follows_the_bundle_layout() {
        assert_eq!(
            sidecar_dir(Path::new("/Applications/OpenWork.app")),
            PathBuf::from("/Applications/OpenWork.app/Contents/Resources/sidecars")
        );
        assert_eq!(
            sidecar_dir(Path::new("/opt/openwork/OpenWork.exe")),
            PathBuf::from("/opt/openwork/resources/sidecars")
        );
        assert_eq!(
            sidecar_dir(Path::new("/opt/openwork")),
            PathBuf::from("/opt/openwork/resources/sidecars")
        );
    }

    #[test]
    fn pick_returns_the_first_sidecar_that_exists() {
        let dir = util::temp_dir("sidecar-pick").expect("a temp dir");
        std::fs::write(dir.join("opencode2"), "x").expect("a fixture sidecar");

        assert_eq!(pick(&dir, Engine::V2), Some(dir.join("opencode2")));
        assert_eq!(pick(&dir, Engine::V1), None);

        util::remove_dir(&dir);
    }

    #[test]
    fn a_missing_engine_is_a_skip_with_a_readable_detail() {
        let result = skipped("the OpenWork sidecars are absent");

        assert!(!result.ok);
        assert!(result.skipped);
        assert_eq!(result.error, None);
        assert!(result
            .detail
            .expect("a skip carries a detail")
            .contains("the OpenWork sidecars are absent"));
    }

    #[test]
    fn parses_a_version_out_of_the_sidecar_banner() {
        assert_eq!(parse_version("opencode2 2.2.0"), Some("2.2.0".to_string()));
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn failure_and_detail_stay_readable() {
        let result = failed("boom", None);
        assert!(!result.ok);
        assert_eq!(result.error.as_deref(), Some("boom"));
        assert_eq!(result.detail, None);

        assert_eq!(truncate("OK", DETAIL_LIMIT), "OK");
        let long = "x".repeat(DETAIL_LIMIT + 10);
        let cut = truncate(&long, DETAIL_LIMIT);
        assert_eq!(cut.chars().count(), DETAIL_LIMIT + 3);
        assert!(cut.ends_with("..."));
    }

    #[test]
    fn masks_a_token_that_reaches_the_error() {
        // Fixture: the first stderr line of `opencode run` can quote the key.
        let stderr = format!("HTTP 401 for key {FIXTURE_TOKEN}");

        let result = failed(
            format!("the {PROVIDER} check failed: {stderr}"),
            Some(FIXTURE_TOKEN),
        );

        let message = result.error.expect("the failure carries an error");
        assert!(message.contains(util::MASK), "masked message: {message}");
        assert!(
            !message.contains(FIXTURE_TOKEN),
            "leaked message: {message}"
        );
    }

    #[test]
    fn masks_a_short_token_and_keeps_the_rest() {
        let result = failed("HTTP 401 for key k9z", Some("k9z"));

        assert_eq!(result.error.as_deref(), Some("HTTP 401 for key ***"));
    }

    #[test]
    fn without_a_token_the_message_stays_unchanged() {
        assert_eq!(failed("boom", None).error.as_deref(), Some("boom"));
        assert_eq!(failed("boom", Some("  ")).error.as_deref(), Some("boom"));
    }

    // Fixture: the multi-line usage banner `opencode run` prints on a bad
    // invocation, the real cause a few lines below, and a key that must stay masked.
    const USAGE_STDERR: &str = "\
opencode run [message..]\n\
\n\
Run opencode with a message\n\
\n\
Positionals:\n\
  message  message to send  [array] [default: []]\n\
\n\
Options:\n\
  --model  model to use in the format of provider/model\n\
\n\
error: model not found for key sk-live-fixture-0123456789\n";

    fn output(stderr: &str, stdout: &str, code: i32) -> util::CommandOutput {
        util::CommandOutput {
            success: false,
            code: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn failure_message_names_the_exit_code_version_and_true_cause() {
        let args = smoke_args();
        let captured = output(USAGE_STDERR, "stdout noise\n", 1);

        let message = failure_message(&args, "2.2.3", &captured);
        let message = util::mask_secret(&message, Some(FIXTURE_TOKEN));

        assert!(message.contains("usage"), "shape missing: {message}");
        assert!(
            message.contains("--standalone") && message.contains("--model"),
            "args missing: {message}"
        );
        assert!(message.contains("exit code: 1"), "code missing: {message}");
        assert!(
            message.contains("opencode version: 2.2.3"),
            "version missing: {message}"
        );
        assert!(
            message.contains("model not found"),
            "true cause missing: {message}"
        );
        assert!(
            message.contains("stdout noise"),
            "stdout lines missing: {message}"
        );
        assert!(message.contains(util::MASK), "mask missing: {message}");
        assert!(!message.contains(FIXTURE_TOKEN), "token leaked: {message}");
    }

    #[test]
    fn failure_message_marks_a_runtime_error_and_drops_blank_lines() {
        let captured = output("\n\nerror: HTTP 401 unauthorized\n\n", "", 2);

        let message = failure_message(&smoke_args(), "unknown", &captured);

        assert!(
            message.contains("runtime error"),
            "runtime shape missing: {message}"
        );
        assert!(!message.contains("usage"), "wrong shape: {message}");
        assert!(message.contains("error: HTTP 401 unauthorized"));
        assert!(!message.contains("\n\n\n"), "blank lines kept: {message}");
    }

    #[test]
    fn output_lines_stop_at_the_limit_and_spread_across_both_streams() {
        let stderr = (1..=8)
            .map(|index| format!("err {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let stdout = (1..=8)
            .map(|index| format!("out {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let captured = output(&stderr, &stdout, 1);

        let lines = meaningful_lines(&captured);

        assert_eq!(lines.len(), OUTPUT_LINE_LIMIT);
        assert_eq!(lines.first().map(String::as_str), Some("err 1"));
        assert_eq!(lines.last().map(String::as_str), Some("out 2"));
    }

    // Recorded on opencode 2.2.5: the help block goes to stdout and the cause to stderr.
    #[test]
    fn usage_dump_on_stdout_reads_as_usage_with_the_real_cause_first() {
        let stderr = "\nERROR\n  Unrecognized flag: --nope in command opencode run\n";
        let stdout = "DESCRIPTION\n  Run OpenCode with a message\n\nUSAGE\n  opencode run [flags] [<message...>]\n";

        let message = failure_message(&smoke_args(), "2.2.5", &output(stderr, stdout, 1));

        assert!(
            message.contains("usage or help printed"),
            "shape missing: {message}"
        );
        assert!(
            message
                .lines()
                .next()
                .is_some_and(|line| line.contains("Unrecognized flag: --nope")),
            "cause missing from the first line: {message}"
        );
        assert!(message.contains("exit code: 1"), "code missing: {message}");
    }

    #[test]
    fn model_error_keeps_its_text_and_drops_color_escapes() {
        let stderr = "\u{1b}[91m\u{1b}[1mError: \u{1b}[0mModel unavailable: nope/nope\n";

        let message = failure_message(&smoke_args(), "2.2.5", &output(stderr, "", 1));

        assert!(
            message.contains("Model unavailable: nope/nope"),
            "cause missing: {message}"
        );
        assert!(!message.contains('\u{1b}'), "escape kept: {message}");
        assert!(
            message.contains("runtime error"),
            "shape missing: {message}"
        );
        assert!(!message.contains("usage"), "wrong shape: {message}");
    }
}
