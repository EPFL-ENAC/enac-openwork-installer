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
pub async fn smoke_test() -> Result<SmokeResult, AppError> {
    let Some(binary) = opencode::binary_path() else {
        return Ok(failed("OpenCode is not installed, run the OpenCode step first"));
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
            return Ok(failed(format!(
                "the {PROVIDER} check failed: {}",
                error.message
            )))
        }
    };
    if reply.is_empty() {
        return Ok(failed(format!("{MODEL} answered with no text")));
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

fn failed(message: impl Into<String>) -> SmokeResult {
    SmokeResult {
        ok: false,
        detail: None,
        error: Some(message.into()),
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

    #[test]
    fn failure_and_detail_stay_readable() {
        let result = failed("boom");
        assert!(!result.ok);
        assert_eq!(result.error.as_deref(), Some("boom"));
        assert_eq!(result.detail, None);

        assert_eq!(truncate("OK", DETAIL_LIMIT), "OK");
        let long = "x".repeat(DETAIL_LIMIT + 10);
        let cut = truncate(&long, DETAIL_LIMIT);
        assert_eq!(cut.chars().count(), DETAIL_LIMIT + 3);
        assert!(cut.ends_with("..."));
    }
}
