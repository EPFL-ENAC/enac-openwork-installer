use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokeResult {
    pub ok: bool,
    /// The engine was not found or not extractable, so the check did not run.
    pub skipped: bool,
    pub detail: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalForm {
    pub units: Vec<String>,
    pub existing_open_work_key: bool,
}

/// One of the user's own portal keys, as the key card offers it. The token is never part
/// of the list: a chosen key is revealed on demand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalKeyChoice {
    pub id: i64,
    pub name: String,
    pub pool: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedKey {
    pub name: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWorkState {
    pub broken: bool,
    pub databases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateRepair {
    pub backups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackendLog {
    pub task: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: String,
    pub message: String,
}

impl AppError {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new("io", message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new("network", message)
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new("parse", message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_with_camel_case_keys() {
        let status = InstallStatus {
            installed: false,
            version: None,
            path: None,
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "installed": false, "version": null, "path": null })
        );

        let form = PortalForm {
            units: vec!["ENAC".into()],
            existing_open_work_key: true,
        };
        assert_eq!(
            serde_json::to_value(&form).unwrap(),
            json!({ "units": ["ENAC"], "existingOpenWorkKey": true })
        );
    }
}
