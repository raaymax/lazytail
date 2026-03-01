use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(not(test))]
use std::fs;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

/// Maximum number of context entries to keep in the session file.
const MAX_CONTEXTS: usize = 100;

/// Key used for non-project (global) context.
const GLOBAL_KEY: &str = "__global__";

#[derive(Debug, Serialize, Deserialize, Default)]
struct SessionFile {
    contexts: HashMap<String, ContextEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextEntry {
    last_source: String,
}

#[cfg(not(test))]
fn session_file_path() -> Option<PathBuf> {
    crate::source::lazytail_dir().map(|p| p.join("session.json"))
}

fn context_key(project_root: Option<&Path>) -> String {
    match project_root {
        Some(p) => p.display().to_string(),
        None => GLOBAL_KEY.to_string(),
    }
}

/// Load the last active source name for the given project context.
///
/// In test builds, returns None to avoid reading the user's real session file.
pub fn load_last_source(project_root: Option<&Path>) -> Option<String> {
    #[cfg(test)]
    {
        let _ = project_root;
        return None;
    }

    #[cfg(not(test))]
    {
        let path = session_file_path()?;
        if !path.exists() {
            return None;
        }

        let content = fs::read_to_string(&path).ok()?;
        let session: SessionFile = serde_json::from_str(&content).ok()?;
        let key = context_key(project_root);
        session.contexts.get(&key).map(|e| e.last_source.clone())
    }
}

/// Save the last active source name for the given project context.
///
/// In test builds, this is a no-op to avoid corrupting the user's real session file.
pub fn save_last_source(project_root: Option<&Path>, name: &str) {
    #[cfg(test)]
    {
        let _ = (project_root, name);
        return;
    }

    #[cfg(not(test))]
    {
        let Some(path) = session_file_path() else {
            return;
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }

        // Load existing session or start fresh
        let mut session: SessionFile = path
            .exists()
            .then(|| {
                fs::read_to_string(&path)
                    .ok()
                    .and_then(|c| serde_json::from_str(&c).ok())
            })
            .flatten()
            .unwrap_or_default();

        let key = context_key(project_root);
        session.contexts.insert(
            key,
            ContextEntry {
                last_source: name.to_string(),
            },
        );

        // Cap entries to prevent unbounded growth
        if session.contexts.len() > MAX_CONTEXTS {
            // Remove oldest entries (arbitrary since HashMap has no order,
            // but this prevents unbounded growth)
            let excess = session.contexts.len() - MAX_CONTEXTS;
            let keys_to_remove: Vec<String> =
                session.contexts.keys().take(excess).cloned().collect();
            for k in keys_to_remove {
                session.contexts.remove(&k);
            }
        }

        if let Ok(content) = serde_json::to_string_pretty(&session) {
            let _ = fs::write(&path, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_roundtrip() {
        let mut session = SessionFile::default();
        session.contexts.insert(
            "/home/user/project".to_string(),
            ContextEntry {
                last_source: "api-logs".to_string(),
            },
        );
        session.contexts.insert(
            GLOBAL_KEY.to_string(),
            ContextEntry {
                last_source: "system".to_string(),
            },
        );

        let json = serde_json::to_string(&session).unwrap();
        let loaded: SessionFile = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loaded.contexts["/home/user/project"].last_source,
            "api-logs"
        );
        assert_eq!(loaded.contexts[GLOBAL_KEY].last_source, "system");
    }

    #[test]
    fn test_context_key() {
        assert_eq!(context_key(None), GLOBAL_KEY);
        assert_eq!(
            context_key(Some(Path::new("/home/user/project"))),
            "/home/user/project"
        );
    }

    #[test]
    fn test_cap_entries() {
        let mut session = SessionFile::default();
        for i in 0..150 {
            session.contexts.insert(
                format!("/project/{}", i),
                ContextEntry {
                    last_source: format!("source-{}", i),
                },
            );
        }

        // Simulate the cap logic
        if session.contexts.len() > MAX_CONTEXTS {
            let excess = session.contexts.len() - MAX_CONTEXTS;
            let keys_to_remove: Vec<String> =
                session.contexts.keys().take(excess).cloned().collect();
            for k in keys_to_remove {
                session.contexts.remove(&k);
            }
        }

        assert_eq!(session.contexts.len(), MAX_CONTEXTS);
    }
}
