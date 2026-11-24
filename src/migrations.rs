use crate::canonical::{Identity, identify};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: i64 = 3;

#[derive(Clone, Copy)]
pub struct Migration {
    pub target_version: i64,
    pub name: &'static str,
    pub apply: fn(&Transaction<'_>) -> Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        target_version: 1,
        name: "initial_schema",
        apply: migrate_to_v1,
    },
    Migration {
        target_version: 2,
        name: "porter_fts_and_access_tracking",
        apply: migrate_to_v2,
    },
    Migration {
        target_version: 3,
        name: "reconcile_canonical_keys",
        apply: migrate_to_v3,
    },
];

const _: () = assert!(
    !MIGRATIONS.is_empty()
        && MIGRATIONS[MIGRATIONS.len() - 1].target_version == CURRENT_SCHEMA_VERSION
);

struct CreatedBackup {
    path: PathBuf,
    is_fallback: bool,
}

pub fn migrate(conn: &mut Connection, db_path: &Path) -> Result<()> {
    migrate_with(conn, db_path, MIGRATIONS)
}

fn migrate_with(conn: &mut Connection, db_path: &Path, migrations: &[Migration]) -> Result<()> {
    let latest = migrations.last().map_or(0, |m| m.target_version);
    let initial_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if initial_version < 0 {
        bail!("database schema version {initial_version} is invalid");
    }
    if initial_version > latest {
        bail!(
            "database schema version {initial_version} is newer than this artmark build (supports up to schema version {latest})"
        );
    }
    if initial_version == latest {
        return Ok(());
    }

    let backup = if initial_version > 0 && is_on_disk(db_path) {
        Some(create_pre_migration_backup(conn, db_path, initial_version)?)
    } else {
        None
    };

    let mut applied_any = false;
    for migration in migrations
        .iter()
        .filter(|migration| migration.target_version > initial_version)
    {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let locked_version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if locked_version < 0 {
            bail!("database schema version {locked_version} is invalid");
        }
        if locked_version > latest {
            bail!(
                "database schema version {locked_version} is newer than this artmark build (supports up to schema version {latest})"
            );
        }
        if locked_version >= migration.target_version {
            continue;
        }
        if locked_version == 0 {
            let existing_tables: i64 = tx.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='artifacts'",
                [],
                |row| row.get(0),
            )?;
            if existing_tables > 0 {
                bail!(
                    "database contains an existing artifacts table with user_version 0; refusing automatic migration"
                );
            }
        }
        if migration.target_version != locked_version + 1 {
            bail!(
                "non-contiguous schema migration from v{locked_version} to v{}",
                migration.target_version
            );
        }

        let step_result: Result<()> = (|| {
            (migration.apply)(&tx)?;
            verify_foreign_keys(&tx)?;
            verify_fts_integrity(&tx)?;
            tx.pragma_update(None, "user_version", migration.target_version)?;
            tx.commit()?;
            Ok(())
        })();

        if let Err(error) = step_result {
            if let Some(backup) = &backup {
                return Err(error).with_context(|| {
                    format!(
                        "schema migration to v{} ({}) failed; database rolled back and pre-migration backup is preserved at {}",
                        migration.target_version,
                        migration.name,
                        backup.path.display()
                    )
                });
            }
            return Err(error).with_context(|| {
                format!(
                    "schema migration to v{} ({}) failed; database rolled back",
                    migration.target_version, migration.name
                )
            });
        }
        applied_any = true;
    }

    if !applied_any
        && let Some(backup) = backup
        && backup.is_fallback
    {
        let _ = std::fs::remove_file(&backup.path);
    }

    Ok(())
}

fn is_on_disk(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    !raw.is_empty() && raw != ":memory:"
}

fn create_pre_migration_backup(
    conn: &Connection,
    db_path: &Path,
    from_version: i64,
) -> Result<CreatedBackup> {
    let backup = reserve_backup_file(db_path, from_version)?;
    let Some(backup_str) = backup.path.to_str() else {
        let _ = std::fs::remove_file(&backup.path);
        bail!(
            "pre-migration backup path is not valid UTF-8: {}",
            backup.path.display()
        );
    };
    if let Err(error) = conn.execute("VACUUM INTO ?1", [backup_str]) {
        let _ = std::fs::remove_file(&backup.path);
        return Err(error).with_context(|| {
            format!(
                "failed to create pre-migration backup at {}",
                backup.path.display()
            )
        });
    }
    Ok(backup)
}

fn reserve_backup_file(db_path: &Path, from_version: i64) -> Result<CreatedBackup> {
    let base = append_suffix(db_path, &format!(".v{from_version}.bak"));
    match create_restricted_file(&base) {
        Ok(()) => {
            return Ok(CreatedBackup {
                path: base,
                is_fallback: false,
            });
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to reserve pre-migration backup at {}",
                    base.display()
                )
            });
        }
    }

    for _ in 0..8 {
        let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let unique = Uuid::now_v7().simple();
        let fallback = append_suffix(db_path, &format!(".v{from_version}.{stamp}.{unique}.bak"));
        match create_restricted_file(&fallback) {
            Ok(()) => {
                return Ok(CreatedBackup {
                    path: fallback,
                    is_fallback: true,
                });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to reserve pre-migration backup at {}",
                        fallback.display()
                    )
                });
            }
        }
    }
    bail!(
        "failed to reserve a unique pre-migration backup path beside {}",
        db_path.display()
    )
}

fn create_restricted_file(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map(|_| ())
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut os = OsString::from(path.as_os_str());
    os.push(suffix);
    PathBuf::from(os)
}

fn verify_foreign_keys(tx: &Transaction<'_>) -> Result<()> {
    let violation: Option<String> = tx
        .query_row("PRAGMA foreign_key_check", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    if let Some(table) = violation {
        bail!("foreign key violation detected in table '{table}' during migration");
    }
    Ok(())
}

fn verify_fts_integrity(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO artifact_fts(artifact_fts) VALUES('integrity-check')",
        [],
    )
    .context("FTS5 integrity check failed during migration")?;
    Ok(())
}

fn migrate_to_v1(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE artifacts (
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
         CREATE INDEX artifacts_recent ON artifacts(last_seen_at DESC);",
    )?;
    Ok(())
}

fn migrate_to_v2(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "ALTER TABLE artifacts ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE artifacts ADD COLUMN last_accessed_at TEXT;
         CREATE INDEX artifacts_accessed ON artifacts(access_count DESC, last_accessed_at DESC);
         DROP TABLE artifact_fts;
         CREATE VIRTUAL TABLE artifact_fts USING fts5(
           artifact_id UNINDEXED,
           title,
           summary,
           search_text,
           topics,
           entities,
           tags,
           tokenize = 'porter unicode61'
         );",
    )?;
    rebuild_all_fts(tx)?;
    Ok(())
}

struct LegacyArtifactRow {
    id: String,
    canonical_key: String,
    kind: String,
    provider: Option<String>,
    state: String,
    primary_uri: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    search_text: Option<String>,
    source_external_id: Option<String>,
    source_revision: Option<String>,
    source_updated_at: Option<String>,
    retrieval_json: Option<String>,
    source_metadata_json: Option<String>,
    catalog_metadata_json: Option<String>,
    provenance_json: Option<String>,
    is_explicitly_saved: bool,
    access_count: i64,
    last_accessed_at: Option<String>,
    created_at: String,
    updated_at: String,
    last_seen_at: String,
    last_indexed_at: Option<String>,
}

fn migrate_to_v3(tx: &Transaction<'_>) -> Result<()> {
    let rows = {
        let mut stmt = tx.prepare(
            "SELECT id, canonical_key, kind, provider, state, primary_uri, title, summary,
                    search_text, source_external_id, source_revision, source_updated_at,
                    retrieval_json, source_metadata_json, catalog_metadata_json, provenance_json,
                    is_explicitly_saved, access_count, last_accessed_at,
                    created_at, updated_at, last_seen_at, last_indexed_at
             FROM artifacts
             ORDER BY created_at ASC, id ASC",
        )?;
        stmt.query_map([], |row| {
            Ok(LegacyArtifactRow {
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
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };

    if rows.is_empty() {
        return Ok(());
    }

    let mut groups: BTreeMap<String, (Identity, Vec<LegacyArtifactRow>)> = BTreeMap::new();
    for row in rows {
        let locator = match row.primary_uri.as_deref() {
            Some(uri) if !uri.trim().is_empty() => Some(uri.to_owned()),
            _ => tx
                .query_row(
                    "SELECT uri FROM aliases WHERE artifact_id=?1 ORDER BY first_seen_at ASC, uri ASC LIMIT 1",
                    [&row.id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?,
        };
        let Some(uri) = locator else {
            continue;
        };
        let Ok(identity) = identify(&uri) else {
            continue;
        };
        groups
            .entry(identity.canonical_key.clone())
            .or_insert_with(|| (identity, Vec::new()))
            .1
            .push(row);
    }

    let mut changed = false;
    let mut pending_key_updates: Vec<(String, Identity)> = Vec::new();

    for (target_key, (identity, mut members)) in groups {
        if members.len() == 1 {
            let row = &members[0];
            if row.canonical_key != target_key
                || row.kind != identity.kind
                || row.provider != identity.provider
                || row.source_external_id != identity.external_id
            {
                pending_key_updates.push((row.id.clone(), identity));
                changed = true;
            }
            continue;
        }

        // Choose a deterministic winner that preserves indexed/explicit catalog work.
        members.sort_by(|a, b| {
            (
                a.state != "indexed",
                !a.is_explicitly_saved,
                a.canonical_key != target_key,
                a.created_at.as_str(),
                a.id.as_str(),
            )
                .cmp(&(
                    b.state != "indexed",
                    !b.is_explicitly_saved,
                    b.canonical_key != target_key,
                    b.created_at.as_str(),
                    b.id.as_str(),
                ))
        });

        let mut winner = members.remove(0);
        for loser in members {
            if winner.state != "indexed" && loser.state == "indexed" {
                winner.state = loser.state;
            }
            if winner.title.is_none() {
                winner.title = loser.title;
            }
            if winner.summary.is_none() {
                winner.summary = loser.summary;
            }
            if winner.search_text.is_none() {
                winner.search_text = loser.search_text;
            }
            if winner.source_revision.is_none() {
                winner.source_revision = loser.source_revision;
            }
            if winner.source_updated_at.is_none() {
                winner.source_updated_at = loser.source_updated_at;
            }
            if winner.retrieval_json.is_none() {
                winner.retrieval_json = loser.retrieval_json;
            }
            if winner.source_metadata_json.is_none() {
                winner.source_metadata_json = loser.source_metadata_json;
            }
            if winner.catalog_metadata_json.is_none() {
                winner.catalog_metadata_json = loser.catalog_metadata_json;
            }
            if winner.provenance_json.is_none() {
                winner.provenance_json = loser.provenance_json;
            }
            winner.is_explicitly_saved |= loser.is_explicitly_saved;
            winner.access_count += loser.access_count;
            winner.last_accessed_at = max_opt_str(winner.last_accessed_at, loser.last_accessed_at);
            if loser.created_at < winner.created_at {
                winner.created_at = loser.created_at;
            }
            if loser.updated_at > winner.updated_at {
                winner.updated_at = loser.updated_at;
            }
            if loser.last_seen_at > winner.last_seen_at {
                winner.last_seen_at = loser.last_seen_at;
            }
            winner.last_indexed_at = max_opt_str(winner.last_indexed_at, loser.last_indexed_at);

            tx.execute(
                "UPDATE aliases SET artifact_id=?1 WHERE artifact_id=?2",
                params![winner.id, loser.id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO topics(artifact_id, topic)
                 SELECT ?1, topic FROM topics WHERE artifact_id=?2",
                params![winner.id, loser.id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO entities(artifact_id, entity)
                 SELECT ?1, entity FROM entities WHERE artifact_id=?2",
                params![winner.id, loser.id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO tags(artifact_id, tag)
                 SELECT ?1, tag FROM tags WHERE artifact_id=?2",
                params![winner.id, loser.id],
            )?;
            tx.execute("DELETE FROM artifact_fts WHERE artifact_id=?1", [&loser.id])?;
            tx.execute("DELETE FROM artifacts WHERE id=?1", [&loser.id])?;
        }

        tx.execute(
            "UPDATE artifacts
             SET state=?2, title=?3, summary=?4, search_text=?5,
                 source_revision=?6, source_updated_at=?7,
                 retrieval_json=?8, source_metadata_json=?9,
                 catalog_metadata_json=?10, provenance_json=?11,
                 is_explicitly_saved=?12, access_count=?13,
                 last_accessed_at=?14, created_at=?15, updated_at=?16,
                 last_seen_at=?17, last_indexed_at=?18
             WHERE id=?1",
            params![
                winner.id,
                winner.state,
                winner.title,
                winner.summary,
                winner.search_text,
                winner.source_revision,
                winner.source_updated_at,
                winner.retrieval_json,
                winner.source_metadata_json,
                winner.catalog_metadata_json,
                winner.provenance_json,
                winner.is_explicitly_saved,
                winner.access_count,
                winner.last_accessed_at,
                winner.created_at,
                winner.updated_at,
                winner.last_seen_at,
                winner.last_indexed_at,
            ],
        )?;
        pending_key_updates.push((winner.id, identity));
        changed = true;
    }

    // Two-phase canonical_key update avoids transient UNIQUE collisions if any
    // rows swap keys.
    for (index, (id, _)) in pending_key_updates.iter().enumerate() {
        tx.execute(
            "UPDATE artifacts SET canonical_key=?2 WHERE id=?1",
            params![id, format!("__migrating_v3__:{index}:{id}")],
        )?;
    }
    for (id, identity) in pending_key_updates {
        tx.execute(
            "UPDATE artifacts
             SET canonical_key=?2, kind=?3, provider=?4, source_external_id=?5
             WHERE id=?1",
            params![
                id,
                identity.canonical_key,
                identity.kind,
                identity.provider,
                identity.external_id,
            ],
        )?;
    }

    if changed {
        tx.execute("DELETE FROM artifact_fts", [])?;
        rebuild_all_fts(tx)?;
    }
    Ok(())
}

fn max_opt_str(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn rebuild_all_fts(tx: &Transaction<'_>) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_db() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("artmark-migrate-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("artmark.db")
    }

    fn create_v1_fixture(path: &Path) {
        let mut conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
        )
        .unwrap();
        migrate_with(&mut conn, path, &MIGRATIONS[..1]).unwrap();
        conn.execute_batch(
            "INSERT INTO artifacts (
               id, canonical_key, kind, provider, state, primary_uri, title, summary, search_text,
               source_external_id, is_explicitly_saved, created_at, updated_at, last_seen_at, last_indexed_at
             ) VALUES (
               'art_v1_fixture',
               'gdrive:doc:LEGACY1',
               'google_doc',
               'google_drive',
               'indexed',
               'https://docs.google.com/document/d/LEGACY1/edit',
               'Webhook Retries RFC',
               'Design for webhook retries and exponential backoff',
               'webhook retries backoff migration identity provisioning',
               'LEGACY1',
               1,
               '2026-06-01T00:00:00.000Z',
               '2026-06-02T00:00:00.000Z',
               '2026-06-03T00:00:00.000Z',
               '2026-06-02T00:00:00.000Z'
             );
             INSERT INTO aliases (uri, artifact_id, first_seen_at, last_seen_at)
             VALUES ('https://docs.google.com/document/d/LEGACY1/edit', 'art_v1_fixture', '2026-06-01T00:00:00.000Z', '2026-06-03T00:00:00.000Z');
             INSERT INTO topics (artifact_id, topic) VALUES ('art_v1_fixture', 'webhooks');
             INSERT INTO entities (artifact_id, entity) VALUES ('art_v1_fixture', 'Event Bus');
             INSERT INTO tags (artifact_id, tag) VALUES ('art_v1_fixture', 'rfc');
             INSERT INTO artifact_fts (artifact_id, title, summary, search_text, topics, entities, tags)
             VALUES (
               'art_v1_fixture',
               'Webhook Retries RFC',
               'Design for webhook retries and exponential backoff',
               'webhook retries backoff migration identity provisioning',
               'webhooks',
               'Event Bus',
               'rfc'
             );",
        )
        .unwrap();
    }

    #[test]
    fn fresh_database_migrates_cleanly_without_backup_file() {
        let path = temp_db();
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        migrate(&mut conn, &path).unwrap();

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert!(!append_suffix(&path, ".v0.bak").exists());
        assert!(!append_suffix(&path, ".v1.bak").exists());

        let fts_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='artifact_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(fts_sql.contains("porter unicode61"));

        drop(conn);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn upgrades_v1_fixture_creates_backup_preserves_data_and_rebuilds_porter_fts() {
        let path = temp_db();
        create_v1_fixture(&path);

        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        migrate(&mut conn, &path).unwrap();

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);

        let backup = append_suffix(&path, ".v1.bak");
        assert!(
            backup.is_file(),
            "expected pre-migration backup at {backup:?}"
        );
        let backup_conn = Connection::open(&backup).unwrap();
        let backup_version: i64 = backup_conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(backup_version, 1);
        let backup_count: i64 = backup_conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(backup_count, 1);

        let (access_count, last_accessed_at): (i64, Option<String>) = conn
            .query_row(
                "SELECT access_count, last_accessed_at FROM artifacts WHERE id='art_v1_fixture'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(access_count, 0);
        assert_eq!(last_accessed_at, None);

        // Porter stemming works immediately after upgrade without manual reindex.
        let stemmed_hit: String = conn
            .query_row(
                "SELECT artifact_id FROM artifact_fts WHERE artifact_fts MATCH '\"retry\" AND \"migrating\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stemmed_hit, "art_v1_fixture");

        drop(backup_conn);
        drop(conn);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn failed_migration_rolls_back_completely_and_preserves_backup() {
        fn failing_v2(tx: &Transaction<'_>) -> Result<()> {
            tx.execute(
                "ALTER TABLE artifacts ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            bail!("simulated interruption during v2 migration");
        }

        let path = temp_db();
        create_v1_fixture(&path);

        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let broken = [
            MIGRATIONS[0],
            Migration {
                target_version: 2,
                name: "simulated_failure",
                apply: failing_v2,
            },
        ];
        let err = migrate_with(&mut conn, &path, &broken).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("rolled back"), "{msg}");
        assert!(msg.contains(".v1.bak"), "{msg}");

        // Database remains intact at v1 without the partial column.
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        assert!(
            conn.prepare("SELECT access_count FROM artifacts").is_err(),
            "failed migration must roll back DDL changes"
        );
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 1);
        assert!(append_suffix(&path, ".v1.bak").is_file());

        // Subsequent clean migration still succeeds and does not clobber the first backup.
        migrate(&mut conn, &path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);

        drop(conn);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn refuses_newer_schema_version_and_unversioned_existing_tables() {
        let path = temp_db();
        let mut conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        let err = migrate(&mut conn, &path).unwrap_err();
        assert!(err.to_string().contains("newer than this artmark build"));
        assert!(!append_suffix(&path, ".v99.bak").exists());
        drop(conn);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        let path2 = temp_db();
        let mut conn2 = Connection::open(&path2).unwrap();
        conn2
            .execute("CREATE TABLE artifacts (id TEXT PRIMARY KEY)", [])
            .unwrap();
        let err2 = migrate(&mut conn2, &path2).unwrap_err();
        assert!(err2.to_string().contains("user_version 0"));
        drop(conn2);
        fs::remove_dir_all(path2.parent().unwrap()).unwrap();
    }

    #[test]
    fn fallback_backups_have_restricted_permissions_and_unique_full_uuids() {
        let path = temp_db();
        let first = reserve_backup_file(&path, 1).unwrap();
        let second = reserve_backup_file(&path, 1).unwrap();
        let third = reserve_backup_file(&path, 1).unwrap();

        assert!(!first.is_fallback);
        assert!(second.is_fallback);
        assert!(third.is_fallback);
        assert_ne!(first.path, second.path);
        assert_ne!(second.path, third.path);

        for fallback in [&second.path, &third.path] {
            let name = fallback.file_name().unwrap().to_str().unwrap();
            let parts: Vec<_> = name.split('.').collect();
            let uuid_part = parts[parts.len() - 2];
            assert_eq!(uuid_part.len(), 32, "expected full 32-hex UUID in {name}");
            assert!(uuid_part.chars().all(|c| c.is_ascii_hexdigit()));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for reserved in [&first.path, &second.path, &third.path] {
                let mode = fs::metadata(reserved).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "expected 0600 mode on {reserved:?}");
            }
        }

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_v1_openers_upgrade_cleanly_without_duplicate_column_errors() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let path = temp_db();
        create_v1_fixture(&path);

        let workers = 4;
        let barrier = Arc::new(Barrier::new(workers));
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let db_path = path.clone();
                let sync = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut conn = Connection::open(&db_path).unwrap();
                    conn.execute_batch(
                        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
                    )
                    .unwrap();
                    sync.wait();
                    migrate(&mut conn, &db_path).unwrap();
                    let version: i64 = conn
                        .pragma_query_value(None, "user_version", |row| row.get(0))
                        .unwrap();
                    assert_eq!(version, CURRENT_SCHEMA_VERSION);
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let backup = append_suffix(&path, ".v1.bak");
        assert!(backup.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&backup).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let backup_conn = Connection::open(&backup).unwrap();
        let backup_version: i64 = backup_conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(backup_version, 1);

        drop(backup_conn);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
