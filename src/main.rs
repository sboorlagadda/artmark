use anyhow::{Context, Result, anyhow};
use artmark::{ArtifactState, DoctorReport, Error, IndexInput, Registry, SearchFilters};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Parser)]
#[command(
    name = "artmark",
    version,
    about = "Local artifact registry for agents"
)]
struct Cli {
    /// SQLite database path (defaults to .artmark/artmark.db in your home directory)
    #[arg(long, global = true, env = "ARTMARK_DB")]
    database: Option<PathBuf>,
    /// Print machine-readable JSON on stdout
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Register a URL or local path without fetching it, optionally indexing a search card in one step
    Add {
        uri: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        explicit: bool,
        /// JSON search card file path, or '-' to read from stdin
        #[arg(long = "json-input")]
        input: Option<String>,
    },
    /// Read one catalog entry by ID, canonical key, or URI
    Get {
        artifact: String,
        /// Inspect the entry without incrementing its local lookup count
        #[arg(long)]
        no_track: bool,
    },
    /// Add an agent-generated search card to an existing entry
    Index {
        artifact: String,
        #[arg(long = "json-input")]
        input: String,
    },
    /// Search the local catalog without contacting providers
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        created_after: Option<String>,
        #[arg(long)]
        seen_after: Option<String>,
        #[arg(long)]
        indexed_after: Option<String>,
    },
    /// Show recently encountered artifacts
    Recent {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },
    /// Remove a catalog entry; the original artifact is untouched
    Forget { artifact: String },
    /// Rebuild the lexical index from local catalog records
    Reindex,
    /// Check database and search index health
    Doctor,
    /// Add artmark guidance to the current directory's AGENTS.md
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Show counts by state and provider
    Stats,
}

#[derive(Subcommand)]
enum ContextCommand {
    /// Create or refresh the managed artmark guidance section
    Init,
}

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

fn main() {
    if let Err((code, error)) = run() {
        eprintln!("artmark: {error:#}");
        std::process::exit(code);
    }
}

type Outcome<T> = std::result::Result<T, (i32, anyhow::Error)>;

fn run() -> Outcome<()> {
    let cli = Cli::parse();
    if matches!(
        &cli.command,
        Command::Context {
            command: ContextCommand::Init
        }
    ) {
        let report = init_context().map_err(input_error)?;
        if cli.json {
            emit(&report);
        } else {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        return Ok(());
    }
    let path = match cli.database {
        Some(path) => path,
        None => default_database().map_err(input_error)?,
    };
    let mut db = Registry::open(&path).map_err(registry_error)?;
    match cli.command {
        Command::Add {
            uri,
            title,
            explicit,
            input,
        } => {
            let entry = match input {
                Some(path) => {
                    let content = read_input(&path).map_err(input_error)?;
                    Some(serde_json::from_str::<IndexInput>(&content).map_err(input_error)?)
                }
                None => None,
            };
            let has_index = entry.is_some();
            let artifact = db
                .add(&uri, title.as_deref(), explicit, entry)
                .map_err(registry_error)?;
            if cli.json {
                if has_index {
                    emit(&artifact);
                } else {
                    emit(
                        &json!({"id": artifact.id, "canonical_key": artifact.canonical_key, "state": artifact.state}),
                    );
                }
                return Ok(());
            }
            println!(
                "{}\n{}\n{}",
                artifact.id, artifact.canonical_key, artifact.state
            );
        }
        Command::Get { artifact, no_track } => {
            let artifact = db.get(&artifact, !no_track).map_err(registry_error)?;
            if cli.json {
                emit(&artifact);
            } else {
                println!(
                    "{}\n{}\n{}\n{}",
                    artifact.id,
                    artifact.title.as_deref().unwrap_or("(untitled)"),
                    artifact.canonical_key,
                    artifact.primary_uri.as_deref().unwrap_or("")
                );
            }
        }
        Command::Index { artifact, input } => {
            let content = read_input(&input).map_err(input_error)?;
            let entry: IndexInput = serde_json::from_str(&content).map_err(input_error)?;
            let artifact = db.index(&artifact, entry).map_err(registry_error)?;
            if cli.json {
                emit(&artifact);
            } else {
                println!("{}\nindexed", artifact.id);
            }
        }
        Command::Search {
            query,
            limit,
            provider,
            kind,
            topic,
            tag,
            state,
            created_after,
            seen_after,
            indexed_after,
        } => {
            let state = state
                .as_deref()
                .map(ArtifactState::from_str)
                .transpose()
                .map_err(input_error)?;
            let filters = SearchFilters {
                provider: provider.as_deref(),
                kind: kind.as_deref(),
                topic: topic.as_deref(),
                tag: tag.as_deref(),
                state,
                created_after: created_after.as_deref(),
                seen_after: seen_after.as_deref(),
                indexed_after: indexed_after.as_deref(),
            };
            let results = db.search(&query, limit, &filters).map_err(registry_error)?;
            if cli.json {
                emit(&json!({"results": results}));
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                for item in results {
                    println!(
                        "{}\t{}\t{}",
                        item.id,
                        item.title.as_deref().unwrap_or("(untitled)"),
                        item.primary_uri.as_deref().unwrap_or("")
                    );
                }
            }
        }
        Command::Recent {
            limit,
            state,
            provider,
        } => {
            let state = state
                .as_deref()
                .map(ArtifactState::from_str)
                .transpose()
                .map_err(input_error)?;
            let results = db
                .recent(limit, state, provider.as_deref())
                .map_err(registry_error)?;
            if cli.json {
                emit(&json!({"results": results}));
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                for item in results {
                    println!(
                        "{}\t{}\t{}",
                        item.id,
                        item.title.as_deref().unwrap_or("(untitled)"),
                        item.primary_uri.as_deref().unwrap_or("")
                    );
                }
            }
        }
        Command::Forget { artifact } => {
            db.forget(&artifact).map_err(registry_error)?;
            if cli.json {
                emit(&json!({"forgotten": true}));
            } else {
                println!("forgotten");
            }
        }
        Command::Reindex => {
            let count = db.reindex().map_err(registry_error)?;
            if cli.json {
                emit(&json!({"reindexed": count}));
            } else {
                println!("reindexed {count} artifacts");
            }
        }
        Command::Doctor => {
            let database = db.doctor().map_err(registry_error)?;
            let healthy = database.healthy;
            let report = CliDoctorReport {
                database,
                agent_setup: agent_setup_report(),
            };
            if cli.json {
                emit(&report);
            } else {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
            if !healthy {
                return Err((4, anyhow!("database health check failed")));
            }
        }
        Command::Stats => {
            let stats = db.stats().map_err(registry_error)?;
            if cli.json {
                emit(&stats);
            } else {
                println!("{}", serde_json::to_string_pretty(&stats).unwrap());
            }
        }
        Command::Context { .. } => {
            unreachable!("context commands are handled before opening the database")
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CliDoctorReport {
    #[serde(flatten)]
    database: DoctorReport,
    agent_setup: serde_json::Value,
}

fn init_context() -> Result<serde_json::Value> {
    let agents_path = std::env::current_dir()?.join("AGENTS.md");
    if std::fs::symlink_metadata(&agents_path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        anyhow::bail!(
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
    let mut action = if existed { "updated" } else { "created" };

    let updated = if start_count == 0 && end_count == 0 {
        let mut next = original.clone();
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
        next
    } else if start_count == 1 && end_count == 1 {
        let start = original.find(CONTEXT_START).unwrap();
        let end = original.find(CONTEXT_END).unwrap();
        if start >= end
            || !marker_is_on_own_line(&original, start, CONTEXT_START)
            || !marker_is_on_own_line(&original, end, CONTEXT_END)
        {
            anyhow::bail!(
                "{} has malformed artmark context markers; repair or remove the markers before retrying",
                agents_path.display()
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
        next
    } else {
        anyhow::bail!(
            "{} has an incomplete or duplicated artmark context marker pair; repair or remove the markers before retrying",
            agents_path.display()
        );
    };

    if updated == original {
        action = "unchanged";
    } else {
        std::fs::write(&agents_path, updated)
            .with_context(|| format!("cannot write {}", agents_path.display()))?;
    }

    Ok(json!({
        "path": agents_path.display().to_string(),
        "action": action
    }))
}

fn marker_is_on_own_line(contents: &str, position: usize, marker: &str) -> bool {
    let before = &contents[..position];
    let after = &contents[position + marker.len()..];
    (position == 0 || before.ends_with('\n'))
        && (after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n"))
}

fn agent_setup_report() -> serde_json::Value {
    let skill_path = codex_home().map(|home| home.join("skills/artmark/SKILL.md"));
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
    json!({
        "codex_skill_installed": installed,
        "codex_skill_path": skill_path,
        "suggestions": suggestions
    })
}

fn codex_home() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_HOME")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.map(|path| path.join(".codex"))
}

fn default_database() -> Result<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let home = home.context("home directory is unavailable; set ARTMARK_DB or --database")?;
    Ok(home.join(".artmark/artmark.db"))
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content)?;
        Ok(content)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("cannot read {path}"))
    }
}

fn emit<T: Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string(value).expect("serializable output")
    );
}
fn input_error(error: impl Into<anyhow::Error>) -> (i32, anyhow::Error) {
    (6, error.into())
}

fn registry_error(error: Error) -> (i32, anyhow::Error) {
    let code = match &error {
        Error::EmptyQuery => 2,
        Error::NotFound { .. } => 3,
        Error::Database(_) => 4,
        Error::InvalidInput(_) => 6,
    };
    (code, anyhow::Error::new(error))
}
