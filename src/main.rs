use anyhow::{Context, Result, anyhow};
use artmark::{
    ArtifactState, DoctorReport, Error, IndexInput, Registry, SearchFilters,
    workspace::{self, AgentSetupReport},
};
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

fn main() {
    if let Err((code, error)) = run() {
        eprintln!("artmark: {error:#}");
        std::process::exit(code);
    }
}

type Outcome<T> = std::result::Result<T, (i32, anyhow::Error)>;

fn open_registry(database: Option<PathBuf>) -> Outcome<Registry> {
    let path = match database {
        Some(path) => path,
        None => workspace::default_database_path(workspace::current_home_dir().as_deref())
            .map_err(input_error)?,
    };
    Registry::open(&path).map_err(registry_error)
}

fn run() -> Outcome<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Context {
            command: ContextCommand::Init,
        } => {
            let cwd = std::env::current_dir().map_err(input_error)?;
            let report = workspace::init_context_in(&cwd).map_err(input_error)?;
            if cli.json {
                emit(&report);
            } else {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
        }
        Command::Add {
            uri,
            title,
            explicit,
            input,
        } => {
            let mut db = open_registry(cli.database)?;
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
            let mut db = open_registry(cli.database)?;
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
            let mut db = open_registry(cli.database)?;
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
            let db = open_registry(cli.database)?;
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
            let db = open_registry(cli.database)?;
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
            let mut db = open_registry(cli.database)?;
            db.forget(&artifact).map_err(registry_error)?;
            if cli.json {
                emit(&json!({"forgotten": true}));
            } else {
                println!("forgotten");
            }
        }
        Command::Reindex => {
            let mut db = open_registry(cli.database)?;
            let count = db.reindex().map_err(registry_error)?;
            if cli.json {
                emit(&json!({"reindexed": count}));
            } else {
                println!("reindexed {count} artifacts");
            }
        }
        Command::Doctor => {
            let db = open_registry(cli.database)?;
            let database = db.doctor().map_err(registry_error)?;
            let healthy = database.healthy;
            let report = CliDoctorReport {
                database,
                agent_setup: workspace::inspect_current_agent_setup(),
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
            let db = open_registry(cli.database)?;
            let stats = db.stats().map_err(registry_error)?;
            if cli.json {
                emit(&stats);
            } else {
                println!("{}", serde_json::to_string_pretty(&stats).unwrap());
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CliDoctorReport {
    #[serde(flatten)]
    database: DoctorReport,
    agent_setup: AgentSetupReport,
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
