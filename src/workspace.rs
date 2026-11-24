use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const CONTEXT_START: &str = "<!-- artmark:context:start -->";
const CONTEXT_END: &str = "<!-- artmark:context:end -->";
const CONTEXT_GUIDANCE: &str = r#"<!-- artmark:context:start -->
## Using artmark

artmark is a local catalog of artifact pointers and retrieval hints. The original source remains authoritative; artmark does not fetch providers or store source documents.

- Do not automatically register every URL or file path the user pastes.
- When the user explicitly asks to save, remember, bookmark, or artmark a durable artifact, inspect the source with an available tool and save it with a search card in one step using `artmark add <uri> --explicit --json-input - --json`. If the source cannot be inspected, fall back to `artmark add <uri> --explicit --json` (with `--title` when known) and tell the user it was saved as a pointer only and is not searchable by topic until indexed.
- Keep source facts separate from your summary, and do not copy source content or secrets.
- Whenever the task requires identifying a durable artifact from a description without an exact locator, use `artmark search <query> --json` before provider search, even if the provider is named. Inspect likely matches with `artmark get <id> --json`, then retrieve current details through the provider.
- At the end of a substantive task, briefly ask whether the user wants to remember any notable durable artifacts read during the task (whether user-supplied or discovered) that are not already indexed. Save and index only those the user approves.
- For a confirmed supersession relationship, keep a brief direct predecessor or successor note in each registered artifact's summary. Do not use special relation tags or accumulate version history.

<!-- artmark:context:end -->
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextAction {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextReport {
    pub path: String,
    pub action: ContextAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSetupReport {
    pub codex_skill_installed: bool,
    pub codex_skill_path: Option<String>,
    pub suggestions: Vec<String>,
}

pub fn current_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn default_database_path(home: Option<&Path>) -> Result<PathBuf> {
    let home = home.context("home directory is unavailable; set ARTMARK_DB or --database")?;
    Ok(home.join(".artmark/artmark.db"))
}

pub fn resolve_codex_home(codex_home_env: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = codex_home_env.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home.map(|path| path.join(".codex"))
}

pub fn merge_context_guidance(original: &str, display_path: &str) -> Result<String> {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let section = if newline == "\n" {
        CONTEXT_GUIDANCE.to_owned()
    } else {
        CONTEXT_GUIDANCE.replace('\n', newline)
    };
    let start_count = original.matches(CONTEXT_START).count();
    let end_count = original.matches(CONTEXT_END).count();

    if start_count == 0 && end_count == 0 {
        let mut next = original.to_owned();
        if !next.is_empty() {
            let blank_line = format!("{newline}{newline}");
            if !next.ends_with(newline) {
                next.push_str(newline);
            }
            if !next.ends_with(&blank_line) {
                next.push_str(newline);
            }
        }
        next.push_str(&section);
        Ok(next)
    } else if start_count == 1 && end_count == 1 {
        let start = original.find(CONTEXT_START).unwrap();
        let end = original.find(CONTEXT_END).unwrap();
        if start >= end
            || !marker_is_on_own_line(original, start, CONTEXT_START)
            || !marker_is_on_own_line(original, end, CONTEXT_END)
        {
            bail!(
                "{display_path} has malformed artmark context markers; repair or remove the markers before retrying"
            );
        }
        let after_end = end + CONTEXT_END.len();
        let suffix = &original[after_end..];
        let section = section.strip_suffix(newline).unwrap_or(&section);
        let mut next = String::with_capacity(original.len() + section.len());
        next.push_str(&original[..start]);
        next.push_str(section);
        if suffix.is_empty() {
            next.push_str(newline);
        }
        next.push_str(suffix);
        Ok(next)
    } else {
        bail!(
            "{display_path} has an incomplete or duplicated artmark context marker pair; repair or remove the markers before retrying"
        );
    }
}

fn marker_is_on_own_line(contents: &str, position: usize, marker: &str) -> bool {
    let before = &contents[..position];
    let after = &contents[position + marker.len()..];
    (position == 0 || before.ends_with('\n'))
        && (after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n"))
}

pub fn init_context_in(dir: &Path) -> Result<ContextReport> {
    let agents_path = dir.join("AGENTS.md");
    if std::fs::symlink_metadata(&agents_path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        bail!(
            "{} is a symbolic link; refusing to edit it",
            agents_path.display()
        );
    }
    let (original, existed) = match std::fs::read_to_string(&agents_path) {
        Ok(contents) => (contents, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", agents_path.display()));
        }
    };

    let display_path = agents_path.display().to_string();
    let updated = merge_context_guidance(&original, &display_path)?;
    let action = if updated == original {
        ContextAction::Unchanged
    } else {
        std::fs::write(&agents_path, updated)
            .with_context(|| format!("cannot write {display_path}"))?;
        if existed {
            ContextAction::Updated
        } else {
            ContextAction::Created
        }
    };

    Ok(ContextReport {
        path: display_path,
        action,
    })
}

pub fn inspect_agent_setup(codex_home: Option<&Path>) -> AgentSetupReport {
    let skill_path = codex_home.map(|home| home.join("skills/artmark/SKILL.md"));
    let installed = skill_path.as_ref().is_some_and(|path| path.is_file());
    let skill_path = skill_path.map(|path| path.display().to_string());
    let suggestions = if installed {
        Vec::new()
    } else {
        vec![
            "Install the artmark Codex skill for the full agent workflow.".to_owned(),
            "Run artmark context init to add project-local artmark guidance to AGENTS.md."
                .to_owned(),
        ]
    };
    AgentSetupReport {
        codex_skill_installed: installed,
        codex_skill_path: skill_path,
        suggestions,
    }
}

pub fn inspect_current_agent_setup() -> AgentSetupReport {
    let codex_env = std::env::var_os("CODEX_HOME");
    let home = current_home_dir();
    let codex_home = resolve_codex_home(codex_env.as_deref(), home.as_deref());
    inspect_agent_setup(codex_home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn merge_context_guidance_handles_fresh_append_crlf_and_malformed_markers() {
        let fresh = merge_context_guidance("", "AGENTS.md").unwrap();
        assert!(fresh.starts_with(CONTEXT_START));
        assert!(fresh.ends_with("\n"));
        assert_eq!(merge_context_guidance(&fresh, "AGENTS.md").unwrap(), fresh);

        let appended = merge_context_guidance("# Repo", "AGENTS.md").unwrap();
        assert!(appended.starts_with("# Repo\n\n<!-- artmark:context:start -->"));
        assert_eq!(
            merge_context_guidance(&appended, "AGENTS.md").unwrap(),
            appended
        );

        let crlf_input = "# Repo\r\n\r\n<!-- artmark:context:start -->\r\nOld\r\n<!-- artmark:context:end -->\r\n\r\nTail\r\n";
        let crlf_updated = merge_context_guidance(crlf_input, "AGENTS.md").unwrap();
        assert!(!crlf_updated.replace("\r\n", "").contains('\n'));
        assert!(crlf_updated.starts_with("# Repo\r\n\r\n"));
        assert!(crlf_updated.ends_with("\r\n\r\nTail\r\n"));

        for bad in [
            "<!-- artmark:context:start -->\nincomplete\n",
            "<!-- artmark:context:start -->\na\n<!-- artmark:context:end -->\n<!-- artmark:context:start -->\nb\n<!-- artmark:context:end -->\n",
            "# Inline <!-- artmark:context:start -->\nx\n<!-- artmark:context:end -->\n",
            "<!-- artmark:context:end -->\nx\n<!-- artmark:context:start -->\n",
        ] {
            assert!(merge_context_guidance(bad, "AGENTS.md").is_err(), "{bad}");
        }
    }

    #[test]
    fn init_context_in_creates_updates_and_rejects_symlinks() {
        let dir = std::env::temp_dir().join(format!("artmark-ws-ctx-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();

        let created = init_context_in(&dir).unwrap();
        assert_eq!(created.action, ContextAction::Created);

        let unchanged = init_context_in(&dir).unwrap();
        assert_eq!(unchanged.action, ContextAction::Unchanged);

        let agents = dir.join("AGENTS.md");
        fs::write(
            &agents,
            "<!-- artmark:context:start -->\nstale\n<!-- artmark:context:end -->\n",
        )
        .unwrap();
        let updated = init_context_in(&dir).unwrap();
        assert_eq!(updated.action, ContextAction::Updated);

        #[cfg(unix)]
        {
            let link_dir = dir.join("linked");
            fs::create_dir_all(&link_dir).unwrap();
            std::os::unix::fs::symlink(&agents, link_dir.join("AGENTS.md")).unwrap();
            let err = init_context_in(&link_dir).unwrap_err();
            assert!(err.to_string().contains("symbolic link"));
        }

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn path_and_setup_helpers_work_without_ambient_env() {
        assert!(default_database_path(None).is_err());
        assert_eq!(
            default_database_path(Some(Path::new("/home/alice"))).unwrap(),
            PathBuf::from("/home/alice/.artmark/artmark.db")
        );

        assert_eq!(
            resolve_codex_home(
                Some(OsStr::new("/custom/codex")),
                Some(Path::new("/home/alice"))
            ),
            Some(PathBuf::from("/custom/codex"))
        );
        assert_eq!(
            resolve_codex_home(Some(OsStr::new("")), Some(Path::new("/home/alice"))),
            Some(PathBuf::from("/home/alice/.codex"))
        );
        assert_eq!(resolve_codex_home(None, None), None);

        let dir = std::env::temp_dir().join(format!("artmark-ws-setup-{}", Uuid::now_v7()));
        let missing = inspect_agent_setup(Some(&dir));
        assert!(!missing.codex_skill_installed);
        assert_eq!(missing.suggestions.len(), 2);

        let skill = dir.join("skills/artmark/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "---\nname: artmark\n---\n").unwrap();
        let present = inspect_agent_setup(Some(&dir));
        assert!(present.codex_skill_installed);
        assert_eq!(
            present.codex_skill_path.as_deref(),
            Some(skill.to_string_lossy().as_ref())
        );
        assert!(present.suggestions.is_empty());

        assert!(!inspect_agent_setup(None).codex_skill_installed);

        fs::remove_dir_all(dir).unwrap();
    }
}
