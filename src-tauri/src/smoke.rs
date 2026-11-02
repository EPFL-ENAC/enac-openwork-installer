use crate::opencode;
use crate::types::{AppError, SmokeResult};
use crate::util;
use std::time::Duration;

pub const PROVIDER: &str = "epfl-rcp-aiaas";
pub const MODEL: &str = "epfl-rcp-aiaas/epfl-aiaas/small";
const TIMEOUT: Duration = Duration::from_secs(60);
const PROMPT: &str = "Reply with the single word OK.";
const DETAIL_LIMIT: usize = 500;

#[tauri::command]
pub async fn smoke_test(token: Option<String>) -> Result<SmokeResult, AppError> {
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
    let output = util::run_checked(
        &binary,
        &["run", "--standalone", "--model", MODEL, PROMPT],
        TIMEOUT,
    )
    .await;
    let reply = match output {
        Ok(output) => output.stdout.trim().to_string(),
        Err(error) => {
            return Ok(failed(
                format!("the {PROVIDER} check failed: {}", error.message),
                secret,
            ))
        }
    };
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
}
