use crate::opencode;
use crate::types::{AppError, SmokeResult};
use crate::util::{self, Logger};
use std::time::Duration;

pub const PROVIDER: &str = "epfl-rcp-aiaas";
pub const MODEL: &str = "epfl-rcp-aiaas/epfl-aiaas/small";
const TIMEOUT: Duration = Duration::from_secs(60);
const PROMPT: &str = "Reply with the single word OK.";
const DETAIL_LIMIT: usize = 500;
const OUTPUT_LINE_LIMIT: usize = 10;

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
) -> Result<SmokeResult, AppError> {
    let secret = token.as_deref();
    let Some(binary) = opencode::binary_path() else {
        return Ok(failed(
            "OpenCode is not installed, run the OpenCode step first",
            secret,
        ));
    };

    // Standalone mode keeps the check off the shared OpenCode server, so a stale
    // background server cannot fail the test. The key comes from the global
    // config, never from the command line.
    let args = smoke_args();
    let version = opencode::version_at(&binary)
        .await
        .unwrap_or_else(|| "unknown".to_string());
    let output = match util::run(&binary, &args, TIMEOUT).await {
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
        detail: Some(format!(
            "{MODEL} answered: {}",
            truncate(&reply, DETAIL_LIMIT)
        )),
        error: None,
    })
}

fn failed(message: impl Into<String>, secret: Option<&str>) -> SmokeResult {
    SmokeResult {
        ok: false,
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
