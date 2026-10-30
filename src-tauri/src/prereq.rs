use crate::util;
use serde::Serialize;
use std::process::Command;

/// Runtime library names, not the deb package names. `ldconfig -p` lists the
/// sonames, so the check works on a machine without the `-dev` packages.
const SONAMES: [(&str, &str); 2] = [
    ("webkit2gtk-4.1", "libwebkit2gtk-4.1.so.0"),
    ("libfuse2", "libfuse.so.2"),
];

const LDCONFIG_CANDIDATES: [&str; 3] = ["ldconfig", "/sbin/ldconfig", "/usr/sbin/ldconfig"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrereqStatus {
    pub missing: Vec<String>,
}

#[tauri::command]
pub fn linux_prereq_status() -> PrereqStatus {
    if util::platform() != "linux" {
        return PrereqStatus {
            missing: Vec::new(),
        };
    }
    // An unavailable ldconfig means the check cannot decide, so it stays silent
    // instead of reporting both libraries as missing.
    let missing = ldconfig_listing()
        .map(|listing| missing_libraries(&listing))
        .unwrap_or_default();
    PrereqStatus { missing }
}

pub fn missing_libraries(listing: &str) -> Vec<String> {
    SONAMES
        .iter()
        .filter(|(_, soname)| !listing.contains(soname))
        .map(|(id, _)| id.to_string())
        .collect()
}

fn ldconfig_listing() -> Option<String> {
    LDCONFIG_CANDIDATES.iter().find_map(|program| {
        let output = Command::new(program).arg("-p").output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reports_nothing_when_both_libraries_are_listed() {
        let listing = "\tlibwebkit2gtk-4.1.so.0 (libc6,x86-64) => /usr/lib/libwebkit2gtk-4.1.so.0\n\
                       \tlibfuse.so.2 (libc6,x86-64) => /usr/lib/libfuse.so.2\n";
        assert!(missing_libraries(listing).is_empty());
    }

    #[test]
    fn reports_webkit_when_only_fuse_is_listed() {
        let listing = "\tlibfuse.so.2 (libc6,x86-64) => /usr/lib/libfuse.so.2\n";
        assert_eq!(missing_libraries(listing), vec!["webkit2gtk-4.1"]);
    }

    #[test]
    fn reports_fuse_when_only_webkit_is_listed() {
        let listing = "\tlibwebkit2gtk-4.1.so.0 (libc6,x86-64) => /usr/lib/libwebkit2gtk-4.1.so.0\n";
        assert_eq!(missing_libraries(listing), vec!["libfuse2"]);
    }

    #[test]
    fn reports_both_when_the_listing_is_empty() {
        assert_eq!(missing_libraries(""), vec!["webkit2gtk-4.1", "libfuse2"]);
    }

    #[test]
    fn serializes_missing_ids_as_camel_case() {
        let status = PrereqStatus {
            missing: vec!["libfuse2".to_string()],
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "missing": ["libfuse2"] })
        );
    }
}
