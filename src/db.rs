use crate::canonical::{self, Identity};
use crate::migrations::{self, CURRENT_SCHEMA_VERSION};
use crate::model::{Artifact, ArtifactState, IndexInput, SearchCard, SearchFilters};
use crate::storage::{self, ARTIFACT_SELECT_COLUMNS, ArtifactRow, FTS_BM25_WEIGHTS};
use anyhow::{Context, bail};
use chrono::Utc;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, params_from_iter};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub enum Error {
    InvalidInput(anyhow::Error),
    EmptyQuery,
    NotFound { key: String },
    Database(anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(err) | Self::Database(err) => fmt::Display::fmt(err, f),
            Self::EmptyQuery => f.write_str("search query cannot be empty"),
            Self::NotFound { .. } => f.write_str("artifact not found"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidInput(err) | Self::Database(err) => err.source(),
            Self::EmptyQuery | Self::NotFound { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub healthy: bool,
    pub sqlite_integrity: String,
    pub foreign_key_integrity: bool,
    pub fts_integrity: bool,
    pub schema_version: i64,
    pub artifacts: i64,
    pub fts_rows: i64,
    pub missing_fts: i64,
    pub indexed_missing_search_text: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopAccessedArtifact {
    pub id: String,
    pub title: Option<String>,
    pub canonical_key: String,
    pub provider: Option<String>,
    pub access_count: i64,
    pub last_accessed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatsReport {
    pub artifacts: i64,
    pub states: BTreeMap<String, i64>,
    pub providers: BTreeMap<String, i64>,
    pub total_accesses: i64,
    pub accessed_artifacts: i64,
    pub top_accessed: Vec<TopAccessedArtifact>,
    pub semantic_search: bool,
}

const STOPWORDS: &[&str] = &[
    "a", "about", "after", "am", "an", "and", "any", "are", "as", "at", "be", "been", "before",
    "being", "between", "but", "by", "can", "could", "did", "do", "does", "during", "find", "for",
    "from", "get", "had", "has", "have", "her", "here", "his", "how", "i", "in", "into", "is",
    "it", "its", "look", "looking", "may", "me", "might", "must", "my", "nor", "of", "off", "on",
    "onto", "open", "or", "our", "out", "over", "please", "search", "shall", "should", "show",
    "so", "some", "than", "that", "the", "their", "them", "there", "these", "they", "this",
    "those", "through", "to", "under", "up", "us", "was", "we", "were", "what", "when", "where",
    "which", "who", "whom", "whose", "why", "will", "with", "would", "yet", "you", "your",
];

pub struct Registry {
    conn: Connection,
    base_dir: Option<PathBuf>,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_disk(path).map_err(Error::Database)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::open_memory().map_err(Error::Database)
    }

    pub fn with_base_dir(mut self, base_dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(base_dir.into());
        self
    }

    fn identify_locator(&self, input: &str) -> anyhow::Result<Identity> {
        match self.base_dir.as_deref() {
            Some(base) => canonical::identify_with_base(input, base),
            None => canonical::identify(input),
        }
    }

    fn open_disk(path: &Path) -> anyhow::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent.file_name().is_some_and(|name| name == ".artmark") {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        let mut conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        migrations::migrate(&mut conn, path)?;
        Ok(Self {
            conn,
            base_dir: None,
        })
    }

    fn open_memory() -> anyhow::Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        migrations::migrate(&mut conn, Path::new(":memory:"))?;
        Ok(Self {
            conn,
            base_dir: None,
        })
    }

    pub fn add(
        &mut self,
        uri: &str,
        title: Option<&str>,
        explicitly_saved: bool,
        index: Option<IndexInput>,
    ) -> Result<Artifact> {
        let identity = self.identify_locator(uri).map_err(Error::InvalidInput)?;
        if let Some(ref input) = index {
            input.validate().map_err(Error::InvalidInput)?;
        }
        self.add_tx(&identity, uri, title, explicitly_saved, index)
            .map_err(Error::Database)
    }

    fn add_tx(
        &mut self,
        identity: &Identity,
        uri: &str,
        title: Option<&str>,
        explicitly_saved: bool,
        index: Option<IndexInput>,
    ) -> anyhow::Result<Artifact> {
        let now = timestamp();
        let tx = self.conn.transaction()?;
        let id = upsert_artifact(&tx, identity, uri, title, explicitly_saved, &now)?;
        if let Some(input) = index {
            let current = load_artifact(&tx, &id)?;
            apply_index(&tx, &current, input, &now)?;
        } else {
            storage::upsert_fts(&tx, &id)?;
        }
        tx.commit()?;
        self.get_optional(&id)?.context("new artifact disappeared")
    }

    pub fn get(&mut self, key: &str, track_access: bool) -> Result<Artifact> {
        let Some(id) = self.resolve_id(key).map_err(Error::Database)? else {
            return Err(Error::NotFound {
                key: key.to_owned(),
            });
        };
        if track_access {
            self.record_access(&id).map_err(Error::Database)?;
        }
        load_artifact(&self.conn, &id).map_err(Error::Database)
    }

    fn record_access(&mut self, id: &str) -> anyhow::Result<()> {
        let now = timestamp();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE artifacts
             SET access_count = access_count + 1,
                 last_accessed_at = ?2
             WHERE id = ?1",
            params![id, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn get_optional(&self, key: &str) -> anyhow::Result<Option<Artifact>> {
        let Some(id) = self.resolve_id(key)? else {
            return Ok(None);
        };
        load_artifact(&self.conn, &id).map(Some)
    }

    pub fn recent(
        &self,
        limit: usize,
        state: Option<ArtifactState>,
        provider: Option<&str>,
    ) -> Result<Vec<Artifact>> {
        self.recent_inner(limit, state, provider)
            .map_err(Error::Database)
    }

    fn recent_inner(
        &self,
        limit: usize,
        state: Option<ArtifactState>,
        provider: Option<&str>,
    ) -> anyhow::Result<Vec<Artifact>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM artifacts
             WHERE (?1 IS NULL OR state = ?1)
               AND (?2 IS NULL OR provider = ?2)
             ORDER BY last_seen_at DESC, id DESC
             LIMIT ?3",
        )?;
        let ids = stmt
            .query_map(params![state, provider, limit as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|id| load_artifact(&self.conn, &id))
            .collect()
    }

    pub fn forget(&mut self, key: &str) -> Result<()> {
        let Some(id) = self.resolve_id(key).map_err(Error::Database)? else {
            return Err(Error::NotFound {
                key: key.to_owned(),
            });
        };
        self.forget_id(&id).map_err(Error::Database)
    }

    fn forget_id(&mut self, id: &str) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM artifact_fts WHERE artifact_id=?1", [id])?;
        tx.execute("DELETE FROM artifacts WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn index(&mut self, key: &str, input: IndexInput) -> Result<Artifact> {
        input.validate().map_err(Error::InvalidInput)?;
        let Some(old) = self.get_optional(key).map_err(Error::Database)? else {
            return Err(Error::NotFound {
                key: key.to_owned(),
            });
        };
        self.index_tx(&old, input).map_err(Error::Database)
    }

    fn index_tx(&mut self, old: &Artifact, input: IndexInput) -> anyhow::Result<Artifact> {
        let now = timestamp();
        let tx = self.conn.transaction()?;
        apply_index(&tx, old, input, &now)?;
        tx.commit()?;
        load_artifact(&self.conn, &old.id)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        filters: &SearchFilters<'_>,
    ) -> Result<Vec<SearchCard>> {
        let query = query.trim();
        if query.is_empty() {
            return Err(Error::EmptyQuery);
        }
        self.search_inner(query, limit, filters)
            .map_err(Error::Database)
    }

    fn search_inner(
        &self,
        query: &str,
        limit: usize,
        filters: &SearchFilters<'_>,
    ) -> anyhow::Result<Vec<SearchCard>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let mut seen = HashSet::new();
        if let Some(id) = self.resolve_id(query)? {
            let mut sql = format!("SELECT {SEARCH_CARD_COLUMNS} FROM artifacts a WHERE a.id = ?");
            let mut query_params = vec![SqlValue::Text(id)];
            append_filter_clauses(&mut sql, &mut query_params, filters, "a");
            let mut stmt = self.conn.prepare(&sql)?;
            if let Some(card) = stmt
                .query_row(params_from_iter(query_params), |row| {
                    search_card_row(row, 1.0)
                })
                .optional()?
            {
                seen.insert(card.id.clone());
                results.push(card);
            }
            if canonical::is_locator_or_key(query) {
                hydrate_topics(&self.conn, &mut results)?;
                return Ok(results);
            }
        }

        if results.len() < limit {
            let mut sql = format!(
                "SELECT {SEARCH_CARD_COLUMNS} FROM artifacts a
                 WHERE (a.title = ? COLLATE NOCASE OR a.source_external_id = ?)"
            );
            let mut query_params = vec![
                SqlValue::Text(query.to_owned()),
                SqlValue::Text(query.to_owned()),
            ];
            append_filter_clauses(&mut sql, &mut query_params, filters, "a");
            append_exclusion_clause(&mut sql, &mut query_params, &seen, "a");
            sql.push_str(" ORDER BY a.id ASC LIMIT ?");
            query_params.push(SqlValue::Integer((limit - results.len()) as i64));
            let mut stmt = self.conn.prepare(&sql)?;
            let cards = stmt
                .query_map(params_from_iter(query_params), |row| {
                    search_card_row(row, 0.95)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for card in cards {
                if seen.insert(card.id.clone()) {
                    results.push(card);
                }
            }
        }

        let terms = query_terms(query);
        if !terms.is_empty() && results.len() < limit {
            let and_query = fts_match_query(&terms, " AND ");
            self.run_fts_pass(
                &and_query,
                limit,
                0.55,
                0.35,
                filters,
                &mut seen,
                &mut results,
            )?;

            if terms.len() > 1 && results.len() < limit {
                let or_query = fts_match_query(&terms, " OR ");
                self.run_fts_pass(
                    &or_query,
                    limit,
                    0.05,
                    0.45,
                    filters,
                    &mut seen,
                    &mut results,
                )?;
            }
        }
        hydrate_topics(&self.conn, &mut results)?;
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_fts_pass(
        &self,
        fts_query: &str,
        limit: usize,
        score_floor: f64,
        score_span: f64,
        filters: &SearchFilters<'_>,
        seen: &mut HashSet<String>,
        results: &mut Vec<SearchCard>,
    ) -> anyhow::Result<()> {
        if results.len() >= limit {
            return Ok(());
        }
        let mut sql = format!(
            "SELECT {SEARCH_CARD_COLUMNS}, {FTS_BM25_WEIGHTS} AS rank
             FROM artifact_fts
             JOIN artifacts a ON a.id = artifact_fts.artifact_id
             WHERE artifact_fts MATCH ?"
        );
        let mut query_params = vec![SqlValue::Text(fts_query.to_owned())];
        append_filter_clauses(&mut sql, &mut query_params, filters, "a");
        append_exclusion_clause(&mut sql, &mut query_params, seen, "a");
        sql.push_str(" ORDER BY rank ASC, a.id ASC LIMIT ?");
        query_params.push(SqlValue::Integer((limit - results.len()) as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let cards = stmt
            .query_map(params_from_iter(query_params), |row| {
                let bm25_raw: f64 = row.get(10)?;
                let score = bm25_to_score(bm25_raw, score_floor, score_span);
                search_card_row(row, score)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for card in cards {
            if seen.insert(card.id.clone()) {
                results.push(card);
            }
        }
        Ok(())
    }

    pub fn reindex(&mut self) -> Result<usize> {
        self.reindex_inner().map_err(Error::Database)
    }

    fn reindex_inner(&mut self) -> anyhow::Result<usize> {
        let tx = self.conn.transaction()?;
        let count = storage::rebuild_all_fts(&tx)?;
        tx.commit()?;
        Ok(count)
    }

    pub fn doctor(&self) -> Result<DoctorReport> {
        self.doctor_inner().map_err(Error::Database)
    }

    fn doctor_inner(&self) -> anyhow::Result<DoctorReport> {
        let integrity: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let schema_version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        let foreign_key_integrity = storage::check_foreign_keys(&self.conn)?.is_none();
        let fts_integrity = storage::check_fts_integrity(&self.conn).is_ok();
        let artifacts: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
        let fts_rows: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM artifact_fts", [], |row| row.get(0))?;
        let missing_fts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM artifacts a WHERE NOT EXISTS
             (SELECT 1 FROM artifact_fts f WHERE f.artifact_id=a.id)",
            [],
            |row| row.get(0),
        )?;
        let missing_search_text: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE state=?1 AND (search_text IS NULL OR trim(search_text)='')",
            [ArtifactState::Indexed],
            |row| row.get(0),
        )?;
        let healthy = integrity == "ok"
            && schema_version == CURRENT_SCHEMA_VERSION
            && foreign_key_integrity
            && fts_integrity
            && fts_rows == artifacts
            && missing_fts == 0
            && missing_search_text == 0;
        Ok(DoctorReport {
            healthy,
            sqlite_integrity: integrity,
            foreign_key_integrity,
            fts_integrity,
            schema_version,
            artifacts,
            fts_rows,
            missing_fts,
            indexed_missing_search_text: missing_search_text,
        })
    }

    pub fn stats(&self) -> Result<StatsReport> {
        self.stats_inner().map_err(Error::Database)
    }

    fn stats_inner(&self) -> anyhow::Result<StatsReport> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM artifacts GROUP BY state")?;
        let mut states = BTreeMap::new();
        for row in stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (name, count) = row?;
            states.insert(name, count);
        }
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(provider,'unknown'), COUNT(*) FROM artifacts GROUP BY provider",
        )?;
        let mut providers = BTreeMap::new();
        for row in stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (name, count) = row?;
            providers.insert(name, count);
        }
        let total_accesses: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(access_count), 0) FROM artifacts",
            [],
            |row| row.get(0),
        )?;
        let accessed_artifacts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE access_count > 0",
            [],
            |row| row.get(0),
        )?;
        let mut top_stmt = self.conn.prepare(
            "SELECT id, title, canonical_key, provider, access_count, last_accessed_at
             FROM artifacts
             WHERE access_count > 0
             ORDER BY access_count DESC, last_accessed_at DESC, id ASC
             LIMIT 5",
        )?;
        let top_accessed = top_stmt
            .query_map([], |row| {
                Ok(TopAccessedArtifact {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    canonical_key: row.get(2)?,
                    provider: row.get(3)?,
                    access_count: row.get(4)?,
                    last_accessed_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(StatsReport {
            artifacts: total,
            states,
            providers,
            total_accesses,
            accessed_artifacts,
            top_accessed,
            semantic_search: false,
        })
    }

    fn resolve_id(&self, key: &str) -> anyhow::Result<Option<String>> {
        let direct = self
            .conn
            .query_row(
                "SELECT id FROM artifacts WHERE id=?1 OR canonical_key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        if direct.is_some() {
            return Ok(direct);
        }
        let alias = self
            .conn
            .query_row(
                "SELECT artifact_id FROM aliases WHERE uri=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        if alias.is_some() {
            return Ok(alias);
        }
        if let Ok(Identity { canonical_key, .. }) = self.identify_locator(key) {
            return Ok(self
                .conn
                .query_row(
                    "SELECT id FROM artifacts WHERE canonical_key=?1",
                    [canonical_key],
                    |row| row.get(0),
                )
                .optional()?);
        }
        Ok(None)
    }
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn json_string(value: &Option<Value>) -> anyhow::Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn load_artifact(conn: &Connection, id: &str) -> anyhow::Result<Artifact> {
    let sql = format!("SELECT {ARTIFACT_SELECT_COLUMNS} FROM artifacts WHERE id=?1");
    let mut artifact = conn
        .query_row(&sql, [id], ArtifactRow::from_row)?
        .into_artifact();
    artifact.topics = storage::load_strings(
        conn,
        "SELECT topic FROM topics WHERE artifact_id=?1 ORDER BY topic",
        id,
    )?;
    artifact.entities = storage::load_strings(
        conn,
        "SELECT entity FROM entities WHERE artifact_id=?1 ORDER BY entity",
        id,
    )?;
    artifact.tags = storage::load_strings(
        conn,
        "SELECT tag FROM tags WHERE artifact_id=?1 ORDER BY tag",
        id,
    )?;
    artifact.aliases = storage::load_strings(
        conn,
        "SELECT uri FROM aliases WHERE artifact_id=?1 ORDER BY uri",
        id,
    )?;
    Ok(artifact)
}

fn upsert_artifact(
    tx: &Transaction<'_>,
    identity: &Identity,
    uri: &str,
    title: Option<&str>,
    explicitly_saved: bool,
    now: &str,
) -> anyhow::Result<String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM artifacts WHERE canonical_key=?1",
            [&identity.canonical_key],
            |row| row.get(0),
        )
        .optional()?;
    let id = if let Some(id) = existing {
        tx.execute(
            "UPDATE artifacts SET last_seen_at=?2, updated_at=?2,
             title=COALESCE(title, ?3),
             is_explicitly_saved=MAX(is_explicitly_saved, ?4)
             WHERE id=?1",
            params![id, now, title, explicitly_saved],
        )?;
        id
    } else if let Some(alias_owner) = tx
        .query_row(
            "SELECT artifact_id FROM aliases WHERE uri=?1",
            [uri],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        tx.execute(
            "UPDATE artifacts SET canonical_key=?2, kind=?3, provider=?4,
             source_external_id=COALESCE(?5, source_external_id),
             last_seen_at=?6, updated_at=?6,
             title=COALESCE(title, ?7),
             is_explicitly_saved=MAX(is_explicitly_saved, ?8)
             WHERE id=?1",
            params![
                alias_owner,
                identity.canonical_key,
                identity.kind,
                identity.provider,
                identity.external_id,
                now,
                title,
                explicitly_saved
            ],
        )?;
        alias_owner
    } else {
        let id = format!("art_{}", Uuid::now_v7().simple());
        tx.execute(
            "INSERT INTO artifacts
             (id, canonical_key, kind, provider, state, primary_uri, title,
              source_external_id, is_explicitly_saved, created_at, updated_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?10)",
            params![
                id,
                identity.canonical_key,
                identity.kind,
                identity.provider,
                ArtifactState::Registered,
                identity.uri,
                title,
                identity.external_id,
                explicitly_saved,
                now
            ],
        )?;
        id
    };
    add_alias(tx, uri, &id, now)?;
    Ok(id)
}

fn apply_index(
    tx: &Transaction<'_>,
    old: &Artifact,
    input: IndexInput,
    now: &str,
) -> anyhow::Result<()> {
    let source = input.source_metadata.or(old.source_metadata.clone());
    let title = source
        .as_ref()
        .and_then(|s| s.get("title"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(old.title.clone());
    let revision = source
        .as_ref()
        .and_then(|s| s.get("revision"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(old.source_revision.clone());
    let source_updated_at = source
        .as_ref()
        .and_then(|s| s.get("updated_at"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(old.source_updated_at.clone());
    let retrieval = input.retrieval.or(old.retrieval.clone());
    let provenance = input
        .resolver
        .map(|resolver| json!({"resolver": resolver}))
        .or(old.provenance.clone());
    let catalog_json = serde_json::to_string(&input.catalog)?;
    tx.execute(
        "UPDATE artifacts SET state=?2, title=?3, summary=?4, search_text=?5,
         source_revision=?6, source_updated_at=?7, retrieval_json=?8,
         source_metadata_json=?9, catalog_metadata_json=?10, provenance_json=?11,
         updated_at=?12, last_indexed_at=?12 WHERE id=?1",
        params![
            old.id,
            ArtifactState::Indexed,
            title,
            input.catalog.summary,
            input.catalog.search_text,
            revision,
            source_updated_at,
            json_string(&retrieval)?,
            json_string(&source)?,
            catalog_json,
            json_string(&provenance)?,
            now,
        ],
    )?;
    replace_strings(tx, "topics", "topic", &old.id, &input.catalog.topics)?;
    replace_strings(tx, "entities", "entity", &old.id, &input.catalog.entities)?;
    replace_strings(tx, "tags", "tag", &old.id, &input.catalog.tags)?;
    storage::upsert_fts(tx, &old.id)?;
    Ok(())
}

fn add_alias(tx: &Transaction<'_>, uri: &str, id: &str, now: &str) -> anyhow::Result<()> {
    let owner: Option<String> = tx
        .query_row(
            "SELECT artifact_id FROM aliases WHERE uri=?1",
            [uri],
            |row| row.get(0),
        )
        .optional()?;
    if owner.as_deref().is_some_and(|owner| owner != id) {
        bail!("URI alias already belongs to another artifact");
    }
    tx.execute(
        "INSERT INTO aliases(uri, artifact_id, first_seen_at, last_seen_at)
         VALUES(?1, ?2, ?3, ?3)
         ON CONFLICT(uri) DO UPDATE SET last_seen_at=excluded.last_seen_at",
        params![uri, id, now],
    )?;
    Ok(())
}

fn replace_strings(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    id: &str,
    values: &[String],
) -> anyhow::Result<()> {
    let delete = format!("DELETE FROM {table} WHERE artifact_id=?1");
    let insert = format!("INSERT OR IGNORE INTO {table}(artifact_id, {column}) VALUES(?1, ?2)");
    tx.execute(&delete, [id])?;
    for value in values.iter().map(|v| v.trim()).filter(|v| !v.is_empty()) {
        tx.execute(&insert, params![id, value])?;
    }
    Ok(())
}

const SEARCH_CARD_COLUMNS: &str = "a.id, a.canonical_key, a.title, a.kind, a.provider, a.state, a.summary, a.primary_uri, a.source_updated_at, a.last_indexed_at";

fn search_card_row(row: &Row<'_>, score: f64) -> rusqlite::Result<SearchCard> {
    Ok(SearchCard {
        id: row.get(0)?,
        canonical_key: row.get(1)?,
        title: row.get(2)?,
        kind: row.get(3)?,
        provider: row.get(4)?,
        state: row.get(5)?,
        summary: row.get(6)?,
        primary_uri: row.get(7)?,
        topics: Vec::new(),
        source_updated_at: row.get(8)?,
        last_indexed_at: row.get(9)?,
        score,
    })
}

fn append_filter_clauses(
    sql: &mut String,
    params: &mut Vec<SqlValue>,
    filters: &SearchFilters<'_>,
    alias: &str,
) {
    if let Some(provider) = filters.provider {
        sql.push_str(&format!(" AND {alias}.provider = ?"));
        params.push(SqlValue::Text(provider.to_owned()));
    }
    if let Some(kind) = filters.kind {
        sql.push_str(&format!(" AND {alias}.kind = ?"));
        params.push(SqlValue::Text(kind.to_owned()));
    }
    if let Some(topic) = filters.topic {
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM topics t WHERE t.artifact_id = {alias}.id AND t.topic = ? COLLATE NOCASE)"
        ));
        params.push(SqlValue::Text(topic.to_owned()));
    }
    if let Some(tag) = filters.tag {
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM tags g WHERE g.artifact_id = {alias}.id AND g.tag = ? COLLATE NOCASE)"
        ));
        params.push(SqlValue::Text(tag.to_owned()));
    }
    if let Some(state) = filters.state {
        sql.push_str(&format!(" AND {alias}.state = ?"));
        params.push(SqlValue::Text(state.as_str().to_owned()));
    }
    if let Some(created_after) = filters.created_after {
        sql.push_str(&format!(" AND {alias}.created_at >= ?"));
        params.push(SqlValue::Text(created_after.to_owned()));
    }
    if let Some(seen_after) = filters.seen_after {
        sql.push_str(&format!(" AND {alias}.last_seen_at >= ?"));
        params.push(SqlValue::Text(seen_after.to_owned()));
    }
    if let Some(indexed_after) = filters.indexed_after {
        sql.push_str(&format!(
            " AND {alias}.last_indexed_at IS NOT NULL AND {alias}.last_indexed_at >= ?"
        ));
        params.push(SqlValue::Text(indexed_after.to_owned()));
    }
}

fn append_exclusion_clause(
    sql: &mut String,
    params: &mut Vec<SqlValue>,
    seen: &HashSet<String>,
    alias: &str,
) {
    if seen.is_empty() {
        return;
    }
    let placeholders = vec!["?"; seen.len()].join(", ");
    sql.push_str(&format!(" AND {alias}.id NOT IN ({placeholders})"));
    let mut ids: Vec<&String> = seen.iter().collect();
    ids.sort();
    for id in ids {
        params.push(SqlValue::Text(id.clone()));
    }
}

fn hydrate_topics(conn: &Connection, cards: &mut [SearchCard]) -> anyhow::Result<()> {
    for chunk in cards.chunks_mut(500) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT artifact_id, topic FROM topics WHERE artifact_id IN ({placeholders}) ORDER BY artifact_id, topic"
        );
        let mut stmt = conn.prepare(&sql)?;
        let id_params: Vec<SqlValue> = chunk
            .iter()
            .map(|card| SqlValue::Text(card.id.clone()))
            .collect();
        let mut topics_by_id: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt.query_map(params_from_iter(id_params), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (artifact_id, topic) = row?;
            topics_by_id.entry(artifact_id).or_default().push(topic);
        }
        for card in chunk {
            if let Some(topics) = topics_by_id.remove(&card.id) {
                card.topics = topics;
            }
        }
    }
    Ok(())
}

fn query_terms(query: &str) -> Vec<String> {
    let mut raw = Vec::new();
    let mut seen_raw = HashSet::new();
    for word in query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        let lowered = word.to_lowercase();
        if seen_raw.insert(lowered.clone()) {
            raw.push(lowered);
        }
    }
    let filtered: Vec<String> = raw
        .iter()
        .filter(|word| !is_stopword(word))
        .cloned()
        .collect();
    if filtered.is_empty() { raw } else { filtered }
}

fn is_stopword(word: &str) -> bool {
    STOPWORDS.binary_search(&word).is_ok()
}

fn fts_match_query(terms: &[String], operator: &str) -> String {
    terms
        .iter()
        .map(|word| {
            if word.chars().count() >= 3 {
                format!("(\"{word}\" OR \"{word}\"*)")
            } else {
                format!("\"{word}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(operator)
}

fn bm25_to_score(bm25_raw: f64, floor: f64, span: f64) -> f64 {
    let relevance = if bm25_raw.is_finite() {
        (-bm25_raw).max(0.0)
    } else {
        0.0
    };
    floor + span * (relevance / (relevance + 5.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(db: &Registry) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        [
            "artifacts",
            "aliases",
            "artifact_fts",
            "topics",
            "entities",
            "tags",
        ]
        .into_iter()
        .map(|table| {
            let mut statement = db
                .conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        })
        .collect()
    }

    #[test]
    fn rejected_registrations_leave_empty_and_populated_databases_unchanged() {
        let path = test_path();
        let mut db = Registry::open(&path).unwrap();
        let rejected = crate::credential_cases::rejected();
        for populated in [false, true] {
            if populated {
                let item = db
                    .add(
                        "https://docs.google.com/document/d/ABC/view",
                        None,
                        false,
                        None,
                    )
                    .unwrap();
                db.index(
                    &item.id,
                    catalog("Existing title", "Searchable SCIM migration"),
                )
                .unwrap();
                // A rejected alias must not change title, explicit-save state,
                // timestamps, metadata, related tables, or existing FTS rows.
                db.conn.execute("UPDATE artifacts SET title=NULL, updated_at='before', last_seen_at='before'", []).unwrap();
            }
            let before = snapshot(&db);
            let changes = db.conn.total_changes();
            for uri in &rejected {
                let error = db
                    .add(uri, Some("Must not be stored"), true, None)
                    .unwrap_err();
                assert!(!error.to_string().contains("TEST_SECRET"));
                assert_eq!(db.conn.total_changes(), changes, "{uri}");
                assert_eq!(snapshot(&db), before, "{uri}");
            }
            drop(db);
            db = Registry::open(&path).unwrap();
            assert_eq!(snapshot(&db), before);
        }
        drop(db);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn ordinary_locators_persist_and_query_identity_survives_reopen() {
        let path = test_path();
        let mut db = Registry::open(&path).unwrap();
        for uri in crate::credential_cases::accepted() {
            db.add(&uri, None, false, None).expect(&uri);
        }
        let first = db
            .add(
                "https://example.com/products?code=ABC&state=CA",
                None,
                false,
                None,
            )
            .unwrap();
        let duplicate = db
            .add(
                "https://EXAMPLE.com/products?code=ABC&state=CA#details",
                None,
                false,
                None,
            )
            .unwrap();
        let other = db
            .add(
                "https://example.com/products?code=XYZ&state=CA",
                None,
                false,
                None,
            )
            .unwrap();
        assert_eq!(first.id, duplicate.id);
        assert_ne!(first.id, other.id);
        let before = snapshot(&db);
        drop(db);
        let mut db = Registry::open(&path).unwrap();
        assert_eq!(snapshot(&db), before);
        assert_eq!(
            db.get("https://example.com/products?code=ABC&state=CA", false)
                .unwrap()
                .id,
            first.id
        );
        drop(db);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    fn test_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("artmark-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("artmark.db")
    }

    fn catalog(title: &str, text: &str) -> IndexInput {
        serde_json::from_value(json!({
            "source_metadata": {"title": title, "revision": "3"},
            "catalog": {
                "summary": "An identity migration plan",
                "search_text": text,
                "topics": ["SAML", "onboarding"],
                "entities": ["Project Phoenix"]
            },
            "retrieval": {"provider": "google_drive", "external_id": "ABC"}
        }))
        .unwrap()
    }

    #[test]
    fn aliases_deduplicate_and_survive_reopen() {
        let path = test_path();
        let edit = "https://docs.google.com/document/d/ABC/edit";
        let view = "https://docs.google.com/document/d/ABC/view";
        let mut db = Registry::open(&path).unwrap();
        let first = db.add(edit, None, false, None).unwrap();
        let second = db.add(view, Some("Authentication"), true, None).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.aliases.len(), 2);
        assert!(second.is_explicitly_saved);
        drop(db);
        let mut db = Registry::open(&path).unwrap();
        assert_eq!(db.get(edit, false).unwrap().id, first.id);
        assert_eq!(db.get(view, false).unwrap().id, first.id);
        assert_eq!(db.stats().unwrap().artifacts, 1);
        drop(db);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn fts_updates_rebuilds_and_forgets() {
        let mut db = Registry::open_in_memory().unwrap();
        let item = db
            .add(
                "https://docs.google.com/document/d/ABC/edit",
                None,
                true,
                None,
            )
            .unwrap();
        let filters = SearchFilters::default();
        db.index(
            &item.id,
            catalog(
                "Enterprise Authentication",
                "SAML migration with Okta and SCIM provisioning",
            ),
        )
        .unwrap();
        assert_eq!(db.search("SCIM", 10, &filters).unwrap()[0].id, item.id);
        assert_eq!(
            db.search("onboarding", 10, &filters).unwrap()[0].id,
            item.id
        );
        assert_eq!(
            db.search(
                "SCIM",
                10,
                &SearchFilters {
                    provider: Some("figma"),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            0
        );
        assert_eq!(
            db.search(
                "SAML",
                10,
                &SearchFilters {
                    topic: Some("saml"),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        db.index(
            &item.id,
            catalog(
                "Enterprise Authentication",
                "OAuth migration and token exchange",
            ),
        )
        .unwrap();
        assert!(db.search("SCIM", 10, &filters).unwrap().is_empty());
        assert_eq!(db.reindex().unwrap(), 1);
        assert_eq!(db.search("OAuth", 10, &filters).unwrap().len(), 1);
        assert!(db.doctor().unwrap().healthy);
        db.forget(&item.id).unwrap();
        assert!(db.search("OAuth", 10, &filters).unwrap().is_empty());
        assert!(db.doctor().unwrap().healthy);
    }

    #[test]
    fn add_and_index_is_atomic_deduplicates_and_filters_recent() {
        let mut db = Registry::open_in_memory().unwrap();

        let empty_before = snapshot(&db);
        let invalid_card: IndexInput = serde_json::from_value(json!({
            "source_metadata": {"body": "must reject"},
            "catalog": {"summary": "Bad", "search_text": "Bad"}
        }))
        .unwrap();
        assert!(matches!(
            db.add(
                "https://docs.google.com/document/d/ATOM/edit",
                None,
                true,
                Some(invalid_card),
            ),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(snapshot(&db), empty_before);

        let first = db
            .add(
                "https://docs.google.com/document/d/ATOM/edit",
                Some("Fallback Title"),
                true,
                Some(catalog("Atomic Doc", "SCIM provisioning architecture")),
            )
            .unwrap();
        assert_eq!(first.state, ArtifactState::Indexed);
        assert!(first.is_explicitly_saved);
        assert_eq!(first.title.as_deref(), Some("Atomic Doc"));
        assert_eq!(
            db.search("SCIM", 10, &SearchFilters::default()).unwrap()[0].id,
            first.id
        );

        let minimal_card: IndexInput = serde_json::from_value(json!({
            "catalog": {
                "summary": "Refreshed summary",
                "search_text": "OIDC federation refresh",
                "topics": ["OIDC"]
            }
        }))
        .unwrap();
        let second = db
            .add(
                "https://docs.google.com/document/d/ATOM/view",
                None,
                false,
                Some(minimal_card),
            )
            .unwrap();
        assert_eq!(second.id, first.id);
        assert_eq!(second.aliases.len(), 2);
        assert!(second.is_explicitly_saved);
        assert_eq!(second.title.as_deref(), Some("Atomic Doc"));
        assert_eq!(second.source_revision.as_deref(), Some("3"));
        assert!(second.retrieval.is_some());
        assert_eq!(
            db.search("OIDC", 10, &SearchFilters::default()).unwrap()[0].id,
            first.id
        );

        let before_bad_update = snapshot(&db);
        let changes = db.conn.total_changes();
        let empty_summary: IndexInput = serde_json::from_value(json!({
            "catalog": {"summary": "   ", "search_text": "nonempty"}
        }))
        .unwrap();
        assert!(matches!(
            db.add(
                "https://docs.google.com/document/d/ATOM/preview",
                None,
                true,
                Some(empty_summary),
            ),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            db.add(
                "https://docs.google.com/document/d/ATOM/preview?token=TEST_SECRET",
                None,
                true,
                Some(catalog("Should Fail", "Should Fail")),
            ),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(db.conn.total_changes(), changes);
        assert_eq!(snapshot(&db), before_bad_update);

        let unindexed = db
            .add(
                "https://github.com/org/repo/issues/99",
                Some("Bug 99"),
                true,
                None,
            )
            .unwrap();
        assert_eq!(unindexed.state, ArtifactState::Registered);
        let registered = db
            .recent(10, Some(ArtifactState::Registered), None)
            .unwrap();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].id, unindexed.id);
        let indexed = db.recent(10, Some(ArtifactState::Indexed), None).unwrap();
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].id, first.id);
        let gdrive = db.recent(10, None, Some("google_drive")).unwrap();
        assert_eq!(gdrive.len(), 1);
        assert_eq!(gdrive[0].id, first.id);
        assert!(
            db.recent(10, Some(ArtifactState::Indexed), Some("github"))
                .unwrap()
                .is_empty()
        );
        assert!(db.doctor().unwrap().healthy);
    }

    #[test]
    fn stopwords_list_is_strictly_sorted() {
        assert!(STOPWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn fts_supports_porter_stemming_stopwords_and_first_bm25_and_operator_words() {
        let mut db = Registry::open_in_memory().unwrap();
        let filters = SearchFilters::default();

        let webhook_rfc = db
            .add(
                "https://docs.google.com/document/d/WH_RFC/edit",
                None,
                true,
                Some(catalog(
                    "Webhook Retries RFC",
                    "Design for outbound webhook retries exponential backoff jitter and dead letter queues",
                )),
            )
            .unwrap();
        let webhook_note = db
            .add(
                "https://docs.google.com/document/d/WH_NOTE/edit",
                None,
                true,
                Some(catalog(
                    "Partner Integration Notes",
                    "Mentions webhooks once without delivery details",
                )),
            )
            .unwrap();
        let cert_doc = db
            .add(
                "https://docs.google.com/document/d/CERT_DOC/edit",
                None,
                true,
                Some(catalog(
                    "TLS Certificate Rotation Postmortem",
                    "Root cause for certificate rotation outage near midnight",
                )),
            )
            .unwrap();

        // Porter stemming matches "retry" -> "retries" and "rotating certificates" -> "rotation certificate".
        let retry_hits = db
            .search("where is the retry backoff", 10, &filters)
            .unwrap();
        assert_eq!(retry_hits.len(), 1);
        assert_eq!(retry_hits[0].id, webhook_rfc.id);
        assert!(retry_hits[0].score > 0.55 && retry_hits[0].score < 0.90);

        let cert_hits = db.search("rotating certificates", 10, &filters).unwrap();
        assert_eq!(cert_hits.len(), 1);
        assert_eq!(cert_hits[0].id, cert_doc.id);

        // AND match ranks strictly above OR fallback match with clear score separation.
        let tiered = db.search("webhook backoff", 10, &filters).unwrap();
        assert_eq!(tiered.len(), 2);
        assert_eq!(tiered[0].id, webhook_rfc.id);
        assert_eq!(tiered[1].id, webhook_note.id);
        assert!(tiered[0].score > 0.55);
        assert!(tiered[1].score < 0.50);

        // Reserved FTS5 operator tokens and punctuation-only queries never error.
        let operator_hits = db.search("NOT NEAR AND OR midnight", 10, &filters).unwrap();
        assert_eq!(operator_hits[0].id, cert_doc.id);
        assert!(db.search("--- ???", 10, &filters).unwrap().is_empty());

        // Stopword-only query falls back to raw terms instead of silently producing an empty match expression.
        let stopword_only = db.search("for and", 10, &filters).unwrap();
        assert!(!stopword_only.is_empty());
    }

    #[test]
    fn lookup_tracking_counts_explicit_gets_only_and_surfaces_in_stats() {
        let mut db = Registry::open_in_memory().unwrap();
        let filters = SearchFilters::default();

        let first = db
            .add(
                "https://docs.google.com/document/d/TRACK1/edit",
                None,
                true,
                Some(catalog("First Anchor Doc", "webhook retries architecture")),
            )
            .unwrap();
        let second = db
            .add(
                "https://docs.google.com/document/d/TRACK2/edit",
                None,
                true,
                Some(catalog("Second Anchor Doc", "saml federation architecture")),
            )
            .unwrap();
        let third = db
            .add(
                "https://github.com/org/repo/issues/7",
                Some("Issue 7"),
                true,
                None,
            )
            .unwrap();

        // Internal reads, search, recent, index, reindex, stats, doctor, and --no-track do not increment.
        assert_eq!(first.access_count, 0);
        assert_eq!(first.last_accessed_at, None);
        assert!(!db.search("architecture", 10, &filters).unwrap().is_empty());
        assert_eq!(db.recent(10, None, None).unwrap().len(), 3);
        assert_eq!(db.reindex().unwrap(), 3);
        assert!(db.doctor().unwrap().healthy);
        let no_track = db.get(&first.id, false).unwrap();
        assert_eq!(no_track.access_count, 0);
        assert_eq!(no_track.last_accessed_at, None);

        let initial_stats = db.stats().unwrap();
        assert_eq!(initial_stats.total_accesses, 0);
        assert_eq!(initial_stats.accessed_artifacts, 0);
        assert!(initial_stats.top_accessed.is_empty());

        // Explicit tracked gets increment access_count and set last_accessed_at without mutating updated_at/last_seen_at.
        let updated_before = first.updated_at.clone();
        let seen_before = first.last_seen_at.clone();
        let g1 = db.get(&first.id, true).unwrap();
        assert_eq!(g1.access_count, 1);
        assert!(g1.last_accessed_at.is_some());
        assert_eq!(g1.updated_at, updated_before);
        assert_eq!(g1.last_seen_at, seen_before);

        // Lookup by canonical key or alias URL also increments the same artifact.
        let g2 = db
            .get("https://docs.google.com/document/d/TRACK1/view", true)
            .unwrap();
        assert_eq!(g2.id, first.id);
        assert_eq!(g2.access_count, 2);

        let s1 = db.get(&second.id, true).unwrap();
        assert_eq!(s1.access_count, 1);

        // Untouched artifact remains at 0 and is omitted from top_accessed.
        assert_eq!(db.get(&third.id, false).unwrap().access_count, 0);

        let stats = db.stats().unwrap();
        assert_eq!(stats.total_accesses, 3);
        assert_eq!(stats.accessed_artifacts, 2);
        assert_eq!(stats.top_accessed.len(), 2);
        assert_eq!(stats.top_accessed[0].id, first.id);
        assert_eq!(stats.top_accessed[0].access_count, 2);
        assert_eq!(stats.top_accessed[1].id, second.id);
        assert_eq!(stats.top_accessed[1].access_count, 1);

        // Forgetting removes the artifact from usage stats cleanly.
        db.forget(&first.id).unwrap();
        let after_forget = db.stats().unwrap();
        assert_eq!(after_forget.total_accesses, 1);
        assert_eq!(after_forget.accessed_artifacts, 1);
        assert_eq!(after_forget.top_accessed[0].id, second.id);
    }

    #[test]
    fn local_filesystem_path_search_short_circuits_without_unrelated_fts_hits() {
        let mut db = Registry::open_in_memory().unwrap();
        let filters = SearchFilters::default();

        let local = db
            .add(
                "/workspace/ops/canary-deployment-runbook.md",
                None,
                true,
                Some(catalog(
                    "Canary Deployment Runbook",
                    "Local markdown runbook for progressive canary deployments",
                )),
            )
            .unwrap();
        let _unrelated = db
            .add(
                "https://www.notion.so/Backup-Runbook-0123456789abcdef0123456789abcdef",
                None,
                true,
                Some(catalog(
                    "Postgres Backup Runbook",
                    "Operational runbook for point-in-time recovery and light/dark dashboard theme",
                )),
            )
            .unwrap();

        let exact_path = db
            .search("/workspace/ops/canary-deployment-runbook.md", 10, &filters)
            .unwrap();
        assert_eq!(exact_path.len(), 1);
        assert_eq!(exact_path[0].id, local.id);
        assert_eq!(exact_path[0].score, 1.0);

        // Prose containing a slash and whitespace must still run lexical FTS search.
        let prose = db.search("light/dark theme", 10, &filters).unwrap();
        assert_eq!(prose.len(), 1);
        assert_ne!(prose[0].id, local.id);
    }
}
