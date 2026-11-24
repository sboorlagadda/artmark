use crate::model::{Artifact, ArtifactState};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde_json::Value;

#[cfg(test)]
pub const FTS_COLUMNS: &[&str] = &[
    "artifact_id",
    "title",
    "summary",
    "search_text",
    "topics",
    "entities",
    "tags",
];

pub const FTS_BM25_WEIGHTS: &str = "bm25(artifact_fts, 0.0, 8.0, 3.0, 1.5, 2.5, 2.5, 1.5)";

pub const CREATE_FTS_V1_SQL: &str = "CREATE VIRTUAL TABLE artifact_fts USING fts5(
  artifact_id UNINDEXED, title, summary, search_text, topics, entities, tags
);";

pub const CREATE_FTS_V2_SQL: &str = "CREATE VIRTUAL TABLE artifact_fts USING fts5(
  artifact_id UNINDEXED,
  title,
  summary,
  search_text,
  topics,
  entities,
  tags,
  tokenize = 'porter unicode61'
);";

pub const ARTIFACT_SELECT_COLUMNS: &str =
    "id, canonical_key, kind, provider, state, primary_uri, title, summary,
  search_text, source_external_id, source_revision, source_updated_at,
  retrieval_json, source_metadata_json, catalog_metadata_json, provenance_json,
  is_explicitly_saved, access_count, last_accessed_at,
  created_at, updated_at, last_seen_at, last_indexed_at";

pub struct ArtifactRow {
    pub id: String,
    pub canonical_key: String,
    pub kind: String,
    pub provider: Option<String>,
    pub state: ArtifactState,
    pub primary_uri: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub search_text: Option<String>,
    pub source_external_id: Option<String>,
    pub source_revision: Option<String>,
    pub source_updated_at: Option<String>,
    pub retrieval_json: Option<String>,
    pub source_metadata_json: Option<String>,
    pub catalog_metadata_json: Option<String>,
    pub provenance_json: Option<String>,
    pub is_explicitly_saved: bool,
    pub access_count: i64,
    pub last_accessed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_seen_at: String,
    pub last_indexed_at: Option<String>,
}

impl ArtifactRow {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
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
            retrieval_json: row.get(12)?,
            source_metadata_json: row.get(13)?,
            catalog_metadata_json: row.get(14)?,
            provenance_json: row.get(15)?,
            is_explicitly_saved: row.get(16)?,
            access_count: row.get(17)?,
            last_accessed_at: row.get(18)?,
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
            last_seen_at: row.get(21)?,
            last_indexed_at: row.get(22)?,
        })
    }

    pub fn into_artifact(self) -> Artifact {
        Artifact {
            id: self.id,
            canonical_key: self.canonical_key,
            kind: self.kind,
            provider: self.provider,
            state: self.state,
            primary_uri: self.primary_uri,
            title: self.title,
            summary: self.summary,
            search_text: self.search_text,
            source_external_id: self.source_external_id,
            source_revision: self.source_revision,
            source_updated_at: self.source_updated_at,
            retrieval: parse_json(self.retrieval_json),
            source_metadata: parse_json(self.source_metadata_json),
            catalog_metadata: parse_json(self.catalog_metadata_json),
            provenance: parse_json(self.provenance_json),
            is_explicitly_saved: self.is_explicitly_saved,
            access_count: self.access_count,
            last_accessed_at: self.last_accessed_at,
            topics: Vec::new(),
            entities: Vec::new(),
            tags: Vec::new(),
            aliases: Vec::new(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_seen_at: self.last_seen_at,
            last_indexed_at: self.last_indexed_at,
        }
    }
}

fn parse_json(value: Option<String>) -> Option<Value> {
    value.and_then(|s| serde_json::from_str(&s).ok())
}

pub fn load_strings(conn: &Connection, sql: &str, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    Ok(stmt
        .query_map([id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn upsert_fts(tx: &Transaction<'_>, id: &str) -> Result<()> {
    tx.execute("DELETE FROM artifact_fts WHERE artifact_id=?1", [id])?;
    let (title, summary, search_text): (Option<String>, Option<String>, Option<String>) = tx
        .query_row(
            "SELECT title, summary, search_text FROM artifacts WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
    let topics = load_strings(tx, "SELECT topic FROM topics WHERE artifact_id=?1", id)?.join(" ");
    let entities =
        load_strings(tx, "SELECT entity FROM entities WHERE artifact_id=?1", id)?.join(" ");
    let tags = load_strings(tx, "SELECT tag FROM tags WHERE artifact_id=?1", id)?.join(" ");
    tx.execute(
        "INSERT INTO artifact_fts(artifact_id, title, summary, search_text, topics, entities, tags)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, title, summary, search_text, topics, entities, tags],
    )?;
    Ok(())
}

pub fn rebuild_all_fts(tx: &Transaction<'_>) -> Result<usize> {
    tx.execute("DELETE FROM artifact_fts", [])?;
    let rows = {
        let mut stmt = tx.prepare("SELECT id, title, summary, search_text FROM artifacts")?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut topic_stmt = tx.prepare("SELECT topic FROM topics WHERE artifact_id=?1")?;
    let mut entity_stmt = tx.prepare("SELECT entity FROM entities WHERE artifact_id=?1")?;
    let mut tag_stmt = tx.prepare("SELECT tag FROM tags WHERE artifact_id=?1")?;
    let mut insert_stmt = tx.prepare(
        "INSERT INTO artifact_fts(artifact_id, title, summary, search_text, topics, entities, tags)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;

    let count = rows.len();
    for (id, title, summary, search_text) in rows {
        let topics = topic_stmt
            .query_map([&id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .join(" ");
        let entities = entity_stmt
            .query_map([&id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .join(" ");
        let tags = tag_stmt
            .query_map([&id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .join(" ");
        insert_stmt.execute(params![
            id,
            title,
            summary,
            search_text,
            topics,
            entities,
            tags
        ])?;
    }
    Ok(count)
}

pub fn check_foreign_keys(conn: &Connection) -> Result<Option<String>> {
    conn.query_row("PRAGMA foreign_key_check", [], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(Into::into)
}

pub fn verify_foreign_keys(conn: &Connection) -> Result<()> {
    if let Some(table) = check_foreign_keys(conn)? {
        bail!("foreign key violation detected in table '{table}' during migration");
    }
    Ok(())
}

pub fn check_fts_integrity(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO artifact_fts(artifact_fts) VALUES('integrity-check')",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;
    use std::path::Path;

    #[test]
    fn fts_schema_columns_and_bm25_weights_stay_aligned() {
        let weight_count = FTS_BM25_WEIGHTS.split(',').skip(1).count();
        assert_eq!(weight_count, FTS_COLUMNS.len());

        for ddl in [CREATE_FTS_V1_SQL, CREATE_FTS_V2_SQL] {
            let mut cursor = 0;
            for col in FTS_COLUMNS {
                let offset = ddl[cursor..]
                    .find(col)
                    .unwrap_or_else(|| panic!("missing FTS column {col} in {ddl}"));
                cursor += offset + col.len();
            }
        }
    }

    #[test]
    fn fts_rebuild_and_integrity_checks_work_end_to_end() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::migrate(&mut conn, Path::new(":memory:")).unwrap();

        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO artifacts (
               id, canonical_key, kind, provider, state, primary_uri, title, summary, search_text,
               is_explicitly_saved, created_at, updated_at, last_seen_at, last_indexed_at
             ) VALUES (
               'art_storage_1', 'gdrive:doc:S1', 'google_doc', 'google_drive', 'indexed',
               'https://docs.google.com/document/d/S1/edit', 'Storage Spec', 'Summary',
               'batched fts rebuild test', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO topics (artifact_id, topic) VALUES ('art_storage_1', 'sqlite')",
            [],
        )
        .unwrap();
        upsert_fts(&tx, "art_storage_1").unwrap();
        assert_eq!(rebuild_all_fts(&tx).unwrap(), 1);
        verify_foreign_keys(&tx).unwrap();
        check_fts_integrity(&tx).unwrap();
        tx.commit().unwrap();

        let hit: String = conn
            .query_row(
                "SELECT artifact_id FROM artifact_fts WHERE artifact_fts MATCH '\"sqlite\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hit, "art_storage_1");
    }
}
