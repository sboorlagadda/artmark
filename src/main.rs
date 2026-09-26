mod canonical;
mod db;
mod model;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use db::Registry;
use model::{IndexInput, SearchFilters};
use serde::Serialize;
use serde_json::json;
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "artmark",
    version,
    about = "Local artifact registry for agents"
)]
struct Cli {
    /// SQLite database path (defaults to ~/.artmark/artmark.db)
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
    /// Register a URL or local path without fetching it
    Add {
        uri: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        explicit: bool,
    },
    /// Read one catalog entry by ID, canonical key, or URI
    Get { artifact: String },
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
    },
    /// Remove a catalog entry; the original artifact is untouched
    Forget { artifact: String },
    /// Rebuild the lexical index from local catalog records
    Reindex,
    /// Check database and search index health
    Doctor,
    /// Show counts by state and provider
    Stats,
}

fn main() {
    if let Err((code, error)) = run() {
        eprintln!("artmark: {error:#}");
        std::process::exit(code);
    }
}

type Outcome<T> = std::result::Result<T, (i32, anyhow::Error)>;

fn run() -> Outcome<()> {
    let cli = Cli::parse();
    let path = cli.database.unwrap_or_else(default_database);
    let mut db = Registry::open(&path).map_err(database_error)?;
    match cli.command {
        Command::Add {
            uri,
            title,
            explicit,
        } => {
            canonical::identify(&uri).map_err(input_error)?;
            let artifact = db
                .add(&uri, title.as_deref(), explicit)
                .map_err(database_error)?;
            if cli.json {
                emit(
                    &json!({"id": artifact.id, "canonical_key": artifact.canonical_key, "state": artifact.state}),
                );
            } else {
                println!(
                    "{}\n{}\n{}",
                    artifact.id, artifact.canonical_key, artifact.state
                );
            }
        }
        Command::Get { artifact } => {
            let artifact = db
                .get(&artifact)
                .map_err(database_error)?
                .ok_or_else(|| (3, anyhow!("artifact not found")))?;
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
            entry.validate().map_err(input_error)?;
            let artifact = db
                .index(&artifact, entry)
                .map_err(database_error)?
                .ok_or_else(|| (3, anyhow!("artifact not found")))?;
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
            if query.trim().is_empty() {
                return Err((2, anyhow!("search query cannot be empty")));
            }
            let filters = SearchFilters {
                provider: provider.as_deref(),
                kind: kind.as_deref(),
                topic: topic.as_deref(),
                tag: tag.as_deref(),
                state: state.as_deref(),
                created_after: created_after.as_deref(),
                seen_after: seen_after.as_deref(),
                indexed_after: indexed_after.as_deref(),
            };
            let results = db.search(&query, limit, &filters).map_err(database_error)?;
            if cli.json {
                emit(&json!({"results": results}));
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
        Command::Recent { limit } => {
            let results = db.recent(limit).map_err(database_error)?;
            if cli.json {
                emit(&json!({"results": results}));
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
            if !db.forget(&artifact).map_err(database_error)? {
                return Err((3, anyhow!("artifact not found")));
            }
            if cli.json {
                emit(&json!({"forgotten": true}));
            } else {
                println!("forgotten");
            }
        }
        Command::Reindex => {
            let count = db.reindex().map_err(database_error)?;
            if cli.json {
                emit(&json!({"reindexed": count}));
            } else {
                println!("reindexed {count} artifacts");
            }
        }
        Command::Doctor => {
            let report = db.doctor().map_err(database_error)?;
            if cli.json {
                emit(&report);
            } else {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
            if report.get("healthy").and_then(|v| v.as_bool()) != Some(true) {
                return Err((4, anyhow!("database health check failed")));
            }
        }
        Command::Stats => {
            let stats = db.stats().map_err(database_error)?;
            if cli.json {
                emit(&stats);
            } else {
                println!("{}", serde_json::to_string_pretty(&stats).unwrap());
            }
        }
    }
    Ok(())
}

fn default_database() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".artmark/artmark.db")
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
fn database_error(error: impl Into<anyhow::Error>) -> (i32, anyhow::Error) {
    (4, error.into())
}
