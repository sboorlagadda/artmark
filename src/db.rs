use crate::canonical::{Identity, identify};
use crate::model::{Artifact, IndexInput, SearchCard, SearchFilters};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub struct Registry {
    conn: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
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
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 1 {
            bail!("database schema version {version} is newer than this artmark build");
        }
        if version == 0 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE artifacts (
                   id TEXT PRIMARY KEY,
                   canonical_key TEXT NOT NULL UNIQUE,
                   kind TEXT NOT NULL,
                   provider TEXT,
                   state TEXT NOT NULL CHECK(state IN ('registered','indexed','unreachable')),
                   primary_uri TEXT,
                   title TEXT,
                   summary TEXT,
                   search_text TEXT,
                   source_external_id TEXT,
                   source_revision TEXT,
                   source_updated_at TEXT,
                   retrieval_json TEXT,
                   source_metadata_json TEXT,
                   catalog_metadata_json TEXT,
                   provenance_json TEXT,
                   is_explicitly_saved INTEGER NOT NULL DEFAULT 0,
                   created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL,
                   last_seen_at TEXT NOT NULL,
                   last_indexed_at TEXT
                 );
                 CREATE TABLE aliases (
                   uri TEXT PRIMARY KEY,
                   artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
                   first_seen_at TEXT NOT NULL,
                   last_seen_at TEXT NOT NULL
                 );
                 CREATE TABLE topics (
                   artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
                   topic TEXT NOT NULL,
                   PRIMARY KEY(artifact_id, topic)
                 );
                 CREATE TABLE entities (
                   artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
                   entity TEXT NOT NULL,
                   PRIMARY KEY(artifact_id, entity)
                 );
                 CREATE TABLE tags (
                   artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
                   tag TEXT NOT NULL,
                   PRIMARY KEY(artifact_id, tag)
                 );
                 CREATE VIRTUAL TABLE artifact_fts USING fts5(
                   artifact_id UNINDEXED, title, summary, search_text, topics, entities, tags
                 );
                 CREATE INDEX aliases_artifact_id ON aliases(artifact_id);
                 CREATE INDEX artifacts_recent ON artifacts(last_seen_at DESC);
                 PRAGMA user_version=1;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    pub fn add(
        &mut self,
        uri: &str,
        title: Option<&str>,
        explicitly_saved: bool,
    ) -> Result<Artifact> {
        let identity = identify(uri)?;
        let now = timestamp();
        let tx = self.conn.transaction()?;
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
        } else {
            let id = format!("art_{}", Uuid::now_v7().simple());
            tx.execute(
                "INSERT INTO artifacts
                 (id, canonical_key, kind, provider, state, primary_uri, title,
                  source_external_id, is_explicitly_saved, created_at, updated_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, 'registered', ?5, ?6, ?7, ?8, ?9, ?9, ?9)",
                params![
                    id,
                    identity.canonical_key,
                    identity.kind,
                    identity.provider,
                    identity.uri,
                    title,
                    identity.external_id,
                    explicitly_saved,
                    now
                ],
            )?;
            id
        };
        add_alias(&tx, uri, &id, &now)?;
        update_fts(&tx, &id)?;
        tx.commit()?;
        self.get(&id)?.context("new artifact disappeared")
    }

    pub fn get(&self, key: &str) -> Result<Option<Artifact>> {
        let Some(id) = self.resolve_id(key)? else {
            return Ok(None);
        };
        let mut artifact: Artifact = self.conn.query_row(
            "SELECT id, canonical_key, kind, provider, state, primary_uri, title, summary,
                    search_text, source_external_id, source_revision, source_updated_at,
                    retrieval_json, source_metadata_json, catalog_metadata_json, provenance_json,
                    is_explicitly_saved, created_at, updated_at, last_seen_at, last_indexed_at
             FROM artifacts WHERE id=?1",
            [&id],
            artifact_row,
        )?;
        artifact.topics = strings(
            &self.conn,
            "SELECT topic FROM topics WHERE artifact_id=?1 ORDER BY topic",
            &id,
        )?;
        artifact.entities = strings(
            &self.conn,
            "SELECT entity FROM entities WHERE artifact_id=?1 ORDER BY entity",
            &id,
        )?;
        artifact.tags = strings(
            &self.conn,
            "SELECT tag FROM tags WHERE artifact_id=?1 ORDER BY tag",
            &id,
        )?;
        artifact.aliases = strings(
            &self.conn,
            "SELECT uri FROM aliases WHERE artifact_id=?1 ORDER BY uri",
            &id,
        )?;
        Ok(Some(artifact))
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<Artifact>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM artifacts ORDER BY last_seen_at DESC, id DESC LIMIT ?1")?;
        let ids = stmt
            .query_map([limit as i64], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|id| self.get(&id).map(Option::unwrap))
            .collect()
    }

    pub fn forget(&mut self, key: &str) -> Result<bool> {
        let Some(id) = self.resolve_id(key)? else {
            return Ok(false);
        };
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM artifact_fts WHERE artifact_id=?1", [&id])?;
        tx.execute("DELETE FROM artifacts WHERE id=?1", [&id])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn index(&mut self, key: &str, input: IndexInput) -> Result<Option<Artifact>> {
        let Some(old) = self.get(key)? else {
            return Ok(None);
        };
        input.validate()?;
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
        let now = timestamp();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE artifacts SET state='indexed', title=?2, summary=?3, search_text=?4,
             source_revision=?5, source_updated_at=?6, retrieval_json=?7,
             source_metadata_json=?8, catalog_metadata_json=?9, provenance_json=?10,
             updated_at=?11, last_indexed_at=?11 WHERE id=?1",
            params![
                old.id,
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
        replace_strings(&tx, "topics", "topic", &old.id, &input.catalog.topics)?;
        replace_strings(&tx, "entities", "entity", &old.id, &input.catalog.entities)?;
        replace_strings(&tx, "tags", "tag", &old.id, &input.catalog.tags)?;
        update_fts(&tx, &old.id)?;
        tx.commit()?;
        self.get(&old.id)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        filters: &SearchFilters<'_>,
    ) -> Result<Vec<SearchCard>> {
        let query = query.trim();
        if query.is_empty() {
            bail!("search query cannot be empty");
        }
        let mut results = Vec::new();
        let mut seen = HashSet::new();
        if let Some(artifact) = self.get(query)? {
            push_card(&mut results, &mut seen, artifact, 1.0, filters);
        } else if let Ok(identity) = identify(query)
            && let Some(artifact) = self.get(&identity.canonical_key)?
        {
            push_card(&mut results, &mut seen, artifact, 1.0, filters);
        }
        let mut exact = self.conn.prepare(
            "SELECT id FROM artifacts WHERE title=?1 COLLATE NOCASE OR source_external_id=?1 ORDER BY id LIMIT 100",
        )?;
        let exact_ids = exact
            .query_map([query], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in exact_ids {
            if let Some(artifact) = self.get(&id)? {
                push_card(&mut results, &mut seen, artifact, 0.95, filters);
            }
        }

        let fts_query = lexical_query(query);
        if !fts_query.is_empty() {
            let mut stmt = self.conn.prepare(
                "SELECT artifact_id FROM artifact_fts WHERE artifact_fts MATCH ?1
                 ORDER BY bm25(artifact_fts, 0, 8, 3, 1, 2, 2, 1) LIMIT 500",
            )?;
            let ids = stmt
                .query_map([fts_query], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (rank, id) in ids.into_iter().enumerate() {
                if let Some(artifact) = self.get(&id)? {
                    push_card(
                        &mut results,
                        &mut seen,
                        artifact,
                        0.7 / (rank as f64 + 1.0),
                        filters,
                    );
                }
            }
        }
        results.truncate(limit);
        Ok(results)
    }

    pub fn reindex(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM artifact_fts", [])?;
        let ids = {
            let mut stmt = tx.prepare("SELECT id FROM artifacts")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in &ids {
            update_fts(&tx, id)?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    pub fn doctor(&self) -> Result<Value> {
        let integrity: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let fts_integrity = self
            .conn
            .execute(
                "INSERT INTO artifact_fts(artifact_fts) VALUES('integrity-check')",
                [],
            )
            .is_ok();
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
            "SELECT COUNT(*) FROM artifacts WHERE state='indexed' AND (search_text IS NULL OR trim(search_text)='')",
            [], |row| row.get(0),
        )?;
        let healthy = integrity == "ok"
            && fts_integrity
            && fts_rows == artifacts
            && missing_fts == 0
            && missing_search_text == 0;
        Ok(json!({
            "healthy": healthy,
            "sqlite_integrity": integrity,
            "fts_integrity": fts_integrity,
            "schema_version": 1,
            "artifacts": artifacts,
            "fts_rows": fts_rows,
            "missing_fts": missing_fts,
            "indexed_missing_search_text": missing_search_text
        }))
    }

    pub fn stats(&self) -> Result<Value> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM artifacts GROUP BY state")?;
        let mut states = serde_json::Map::new();
        for row in stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (name, count) = row?;
            states.insert(name, json!(count));
        }
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(provider,'unknown'), COUNT(*) FROM artifacts GROUP BY provider",
        )?;
        let mut providers = serde_json::Map::new();
        for row in stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (name, count) = row?;
            providers.insert(name, json!(count));
        }
        Ok(
            json!({"artifacts": total, "states": states, "providers": providers, "semantic_search": false}),
        )
    }

    fn resolve_id(&self, key: &str) -> Result<Option<String>> {
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
        if let Ok(Identity { canonical_key, .. }) = identify(key) {
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

fn json_string(value: &Option<Value>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn parse_json(value: Option<String>) -> Option<Value> {
    value.and_then(|s| serde_json::from_str(&s).ok())
}

fn artifact_row(row: &Row<'_>) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get(0)?,
        canonical_key: row.get(1)?,
        kind: row.get(2)?,
        provider: row.get(3)?,
        state: row.get(4)?,
        primary_uri: row.get(5)?,
        title: row.get(6)?,
        summary: row.get(7)?,
        search_text: row.get(8)?,
        source_external_id: row.get(9)?,
        source_revision: row.get(10)?,
        source_updated_at: row.get(11)?,
        retrieval: parse_json(row.get(12)?),
        source_metadata: parse_json(row.get(13)?),
        catalog_metadata: parse_json(row.get(14)?),
        provenance: parse_json(row.get(15)?),
        is_explicitly_saved: row.get(16)?,
        topics: Vec::new(),
        entities: Vec::new(),
        tags: Vec::new(),
        aliases: Vec::new(),
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        last_seen_at: row.get(19)?,
        last_indexed_at: row.get(20)?,
    })
}

fn strings(conn: &Connection, sql: &str, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    Ok(stmt
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn add_alias(tx: &Transaction<'_>, uri: &str, id: &str, now: &str) -> Result<()> {
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
) -> Result<()> {
    let delete = format!("DELETE FROM {table} WHERE artifact_id=?1");
    let insert = format!("INSERT OR IGNORE INTO {table}(artifact_id, {column}) VALUES(?1, ?2)");
    tx.execute(&delete, [id])?;
    for value in values.iter().map(|v| v.trim()).filter(|v| !v.is_empty()) {
        tx.execute(&insert, params![id, value])?;
    }
    Ok(())
}

fn update_fts(tx: &Transaction<'_>, id: &str) -> Result<()> {
    tx.execute("DELETE FROM artifact_fts WHERE artifact_id=?1", [id])?;
    let (title, summary, search_text): (Option<String>, Option<String>, Option<String>) = tx
        .query_row(
            "SELECT title, summary, search_text FROM artifacts WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    let topics = strings(tx, "SELECT topic FROM topics WHERE artifact_id=?1", id)?.join(" ");
    let entities = strings(tx, "SELECT entity FROM entities WHERE artifact_id=?1", id)?.join(" ");
    let tags = strings(tx, "SELECT tag FROM tags WHERE artifact_id=?1", id)?.join(" ");
    tx.execute(
        "INSERT INTO artifact_fts(artifact_id, title, summary, search_text, topics, entities, tags)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, title, summary, search_text, topics, entities, tags],
    )?;
    Ok(())
}

fn push_card(
    results: &mut Vec<SearchCard>,
    seen: &mut HashSet<String>,
    artifact: Artifact,
    score: f64,
    filters: &SearchFilters<'_>,
) {
    if filters.allows(&artifact) && seen.insert(artifact.id.clone()) {
        results.push(SearchCard::from_artifact(&artifact, score));
    }
}

fn lexical_query(query: &str) -> String {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\"*"))
        .collect::<Vec<_>>()
        .join(" OR ")
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
                    .add("https://docs.google.com/document/d/ABC/view", None, false)
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
                let error = db.add(uri, Some("Must not be stored"), true).unwrap_err();
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
            db.add(&uri, None, false).expect(&uri);
        }
        let first = db
            .add(
                "https://example.com/products?code=ABC&state=CA",
                None,
                false,
            )
            .unwrap();
        let duplicate = db
            .add(
                "https://EXAMPLE.com/products?code=ABC&state=CA#details",
                None,
                false,
            )
            .unwrap();
        let other = db
            .add(
                "https://example.com/products?code=XYZ&state=CA",
                None,
                false,
            )
            .unwrap();
        assert_eq!(first.id, duplicate.id);
        assert_ne!(first.id, other.id);
        let before = snapshot(&db);
        drop(db);
        let db = Registry::open(&path).unwrap();
        assert_eq!(snapshot(&db), before);
        assert_eq!(
            db.get("https://example.com/products?code=ABC&state=CA")
                .unwrap()
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
        let first = db.add(edit, None, false).unwrap();
        let second = db.add(view, Some("Authentication"), true).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.aliases.len(), 2);
        assert!(second.is_explicitly_saved);
        drop(db);
        let db = Registry::open(&path).unwrap();
        assert_eq!(db.get(edit).unwrap().unwrap().id, first.id);
        assert_eq!(db.get(view).unwrap().unwrap().id, first.id);
        assert_eq!(db.stats().unwrap()["artifacts"], 1);
        drop(db);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn fts_updates_rebuilds_and_forgets() {
        let path = test_path();
        let mut db = Registry::open(&path).unwrap();
        let item = db
            .add("https://docs.google.com/document/d/ABC/edit", None, true)
            .unwrap();
        let filters = SearchFilters::default();
        db.index(
            &item.id,
            catalog(
                "Enterprise Authentication",
                "SAML migration with Okta and SCIM provisioning",
            ),
        )
        .unwrap()
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
        .unwrap()
        .unwrap();
        assert!(db.search("SCIM", 10, &filters).unwrap().is_empty());
        assert_eq!(db.reindex().unwrap(), 1);
        assert_eq!(db.search("OAuth", 10, &filters).unwrap().len(), 1);
        assert_eq!(db.doctor().unwrap()["healthy"], true);
        assert!(db.forget(&item.id).unwrap());
        assert!(db.search("OAuth", 10, &filters).unwrap().is_empty());
        assert_eq!(db.doctor().unwrap()["healthy"], true);
        drop(db);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
