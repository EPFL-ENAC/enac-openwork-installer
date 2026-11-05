use crate::types::{AppError, OpenWorkState, StateRepair};
use crate::util::{self, Logger};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Table that the OpenCode v1 bootstrap expects. A database with other tables
/// and no `session` table makes OpenWork die with
/// "Database is not empty and has no session table".
pub const SESSION_TABLE: &str = "session";
const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
const CREATE_TABLE: &[u8] = b"create table";
const SIDECARS: [&str; 2] = ["-wal", "-shm"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Database {
    pub path: PathBuf,
    pub tables: Vec<String>,
}

impl Database {
    pub fn has_session_table(&self) -> bool {
        self.tables.iter().any(|table| table == SESSION_TABLE)
    }

    pub fn is_foreign(&self) -> bool {
        !self.tables.is_empty() && !self.has_session_table()
    }
}

#[tauri::command]
pub async fn openwork_state_status() -> Result<OpenWorkState, AppError> {
    util::spawn_blocking(state_blocking).await
}

#[tauri::command]
pub async fn openwork_repair_state(app: tauri::AppHandle) -> Result<StateRepair, AppError> {
    let log = Logger::new(&app, "openwork");
    util::spawn_blocking(move || repair_blocking(&log)).await?
}

fn state_blocking() -> OpenWorkState {
    let broken = foreign_databases(&scan_databases(&database_state_dirs()));
    OpenWorkState {
        broken: !broken.is_empty(),
        databases: broken.iter().map(|path| display(path)).collect(),
    }
}

fn repair_blocking(log: &Logger) -> Result<StateRepair, AppError> {
    let broken = foreign_databases(&scan_databases(&database_state_dirs()));
    if broken.is_empty() {
        log.log("no incompatible OpenWork database was found, nothing was changed");
        return Ok(StateRepair { backups: Vec::new() });
    }
    for path in &broken {
        log.log(format!("backing up {}", path.display()));
    }
    let backups = backup_and_clear(&broken, unix_stamp())?;
    log.log(format!(
        "backed up {} file(s), OpenWork creates a new database on its next start",
        backups.len()
    ));
    Ok(StateRepair {
        backups: backups.iter().map(|path| display(path)).collect(),
    })
}

fn display(path: &Path) -> String {
    match util::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Reads the table names from the `CREATE TABLE` statements that SQLite stores
/// in the file. A byte scan is enough here: it needs no SQLite library and no
/// write access to the user database. Ceiling: text inside user data that looks
/// like `CREATE TABLE x` shows up as a table name, which only makes the check
/// more conservative (it never deletes anything by itself).
pub fn sqlite_tables(bytes: &[u8]) -> Vec<String> {
    if !bytes.starts_with(SQLITE_MAGIC) {
        return Vec::new();
    }
    let mut tables: Vec<String> = Vec::new();
    let mut index = 0;
    while let Some(found) = find_ignore_ascii_case(&bytes[index..], CREATE_TABLE) {
        let after = index + found + CREATE_TABLE.len();
        if let Some(name) = table_name(&bytes[after..]) {
            if !name.is_empty() && !name.starts_with("sqlite_") && !tables.contains(&name) {
                tables.push(name);
            }
        }
        index = after;
    }
    tables.sort();
    tables
}

fn find_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|start| {
        haystack[*start..*start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

fn table_name(bytes: &[u8]) -> Option<String> {
    let mut pos = 0;
    let mut word = read_word(bytes, &mut pos)?;
    if word == "if" {
        if read_word(bytes, &mut pos)? != "not" || read_word(bytes, &mut pos)? != "exists" {
            return None;
        }
        word = read_word(bytes, &mut pos)?;
    }
    Some(word)
}

fn read_word(bytes: &[u8], pos: &mut usize) -> Option<String> {
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    let first = *bytes.get(*pos)?;
    let quoted = match first {
        b'"' => Some(b'"'),
        b'`' => Some(b'`'),
        b'[' => Some(b']'),
        _ => None,
    };
    if let Some(close) = quoted {
        *pos += 1;
        let start = *pos;
        while *pos < bytes.len() && bytes[*pos] != close {
            *pos += 1;
        }
        let word = String::from_utf8_lossy(&bytes[start..*pos]).to_lowercase();
        *pos += 1;
        return Some(word);
    }
    let start = *pos;
    while *pos < bytes.len() && !bytes[*pos].is_ascii_whitespace() && bytes[*pos] != b'(' {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[start..*pos]).to_lowercase())
}

/// OpenCode keeps one database per channel under its data directory. macOS has
/// no special case upstream, so `~/.local/share/opencode` is the usual place,
/// and the Application Support path covers older layouts.
pub fn database_state_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(base) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        dirs.push(base.join("opencode"));
    }
    if let Some(home) = util::home_dir() {
        for dir in [
            home.join(".local/share/opencode"),
            home.join("Library/Application Support/opencode"),
        ] {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs
}

pub fn database_candidates(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_db = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("db"));
            if is_db && path.is_file() {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates
}

pub fn scan_databases(dirs: &[PathBuf]) -> Vec<Database> {
    database_candidates(dirs)
        .into_iter()
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            bytes
                .starts_with(SQLITE_MAGIC)
                .then(|| Database {
                    tables: sqlite_tables(&bytes),
                    path,
                })
        })
        .collect()
}

pub fn foreign_databases(databases: &[Database]) -> Vec<PathBuf> {
    databases
        .iter()
        .filter(|database| database.is_foreign())
        .map(|database| database.path.clone())
        .collect()
}

/// Renames each database and its write-ahead files to `<name>.bak-<stamp>`.
/// Nothing is deleted, so the user keeps a copy of the old state.
pub fn backup_and_clear(paths: &[PathBuf], stamp: u64) -> Result<Vec<PathBuf>, AppError> {
    let mut backups = Vec::new();
    for path in paths {
        let mut targets = vec![path.clone()];
        for suffix in SIDECARS {
            let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
            if sidecar.is_file() {
                targets.push(sidecar);
            }
        }
        for target in targets {
            let backup = PathBuf::from(format!("{}.bak-{stamp}", target.display()));
            std::fs::rename(&target, &backup)
                .map_err(|e| AppError::io(format!("cannot back up {}: {e}", target.display())))?;
            backups.push(backup);
        }
    }
    Ok(backups)
}

fn unix_stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    fn read(path: &Path) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openwork-setup-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_table_names_of_a_sqlite_database() {
        assert_eq!(
            sqlite_tables(&read(&fixture("opencode-db-v2.sqlite"))),
            vec!["session_message".to_string(), "session_v2".to_string()]
        );
        assert_eq!(
            sqlite_tables(&read(&fixture("opencode-db-v1.sqlite"))),
            vec![
                "message".to_string(),
                "part".to_string(),
                "session".to_string()
            ]
        );
        assert_eq!(sqlite_tables(b"not a database"), Vec::<String>::new());
        assert_eq!(sqlite_tables(b""), Vec::<String>::new());
    }

    #[test]
    fn flags_a_database_without_the_session_table() {
        let v2 = Database {
            path: PathBuf::from("/tmp/opencode.db"),
            tables: sqlite_tables(&read(&fixture("opencode-db-v2.sqlite"))),
        };
        let v1 = Database {
            path: PathBuf::from("/tmp/opencode.db"),
            tables: sqlite_tables(&read(&fixture("opencode-db-v1.sqlite"))),
        };
        let empty = Database {
            path: PathBuf::from("/tmp/opencode.db"),
            tables: Vec::new(),
        };
        assert!(v2.is_foreign());
        assert!(!v1.is_foreign());
        assert!(v1.has_session_table());
        assert!(!empty.is_foreign());

        let unrelated = Database {
            path: PathBuf::from("/tmp/audit.db"),
            tables: sqlite_tables(&read(&fixture("opencode-db-unrelated.sqlite"))),
        };
        assert!(unrelated.is_foreign());
    }

    #[test]
    fn scans_only_sqlite_databases_in_a_state_directory() {
        let dir = temp_dir("scan");
        std::fs::copy(fixture("opencode-db-v2.sqlite"), dir.join("opencode.db")).unwrap();
        std::fs::copy(fixture("opencode-db-v1.sqlite"), dir.join("other.db")).unwrap();
        std::fs::write(dir.join("notes.txt"), "keep me").unwrap();

        let mut found = scan_databases(std::slice::from_ref(&dir));
        found.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].path, dir.join("opencode.db"));
        assert!(found[0].is_foreign());
        assert_eq!(found[1].path, dir.join("other.db"));
        assert!(found[1].has_session_table());
        assert_eq!(foreign_databases(&found), vec![dir.join("opencode.db")]);

        assert_eq!(database_candidates(&[dir.clone()]).len(), 2);
        assert_eq!(scan_databases(&[dir.join("missing")]), Vec::new());
    }

    #[test]
    fn default_state_dirs_end_with_opencode() {
        let dirs = database_state_dirs();
        assert!(!dirs.is_empty());
        assert!(dirs
            .iter()
            .all(|dir| dir.file_name().is_some_and(|name| name == "opencode")));
    }

    #[test]
    fn backs_up_a_broken_database_instead_of_deleting_it() {
        let dir = temp_dir("backup");
        let db = dir.join("opencode.db");
        let bytes = read(&fixture("opencode-db-v2.sqlite"));
        std::fs::write(&db, &bytes).unwrap();
        std::fs::write(dir.join("opencode.db-wal"), b"wal").unwrap();

        let backups = backup_and_clear(std::slice::from_ref(&db), 1_700_000_000).unwrap();
        assert_eq!(
            backups,
            vec![
                dir.join("opencode.db.bak-1700000000"),
                dir.join("opencode.db-wal.bak-1700000000"),
            ]
        );
        assert!(!db.exists());
        assert!(!dir.join("opencode.db-wal").exists());
        assert_eq!(read(&backups[0]), bytes);

        assert!(backup_and_clear(std::slice::from_ref(&db), 1).is_err());
        assert_eq!(backup_and_clear(&[], 1).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn keeps_the_display_path_short() {
        let Some(home) = util::home_dir() else {
            return;
        };
        assert_eq!(
            display(&home.join(".local/share/opencode/opencode.db")),
            "~/.local/share/opencode/opencode.db"
        );
        assert_eq!(display(Path::new("/elsewhere/x.db")), "/elsewhere/x.db");
    }
}
