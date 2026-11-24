use anyhow::{Context, Result, bail};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: i64 = 2;

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
];

pub fn migrate(conn: &mut Connection, db_path: &Path) -> Result<()> {
    migrate_with(conn, db_path, MIGRATIONS)
}

fn migrate_with(conn: &mut Connection, db_path: &Path, migrations: &[Migration]) -> Result<()> {
    let latest = migrations.last().map_or(0, |m| m.target_version);
    let mut version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 0 {
        bail!("database schema version {version} is invalid");
    }
    if version > latest {
        bail!(
            "database schema version {version} is newer than this artmark build (supports up to schema version {latest})"
        );
    }
    if version == latest {
        return Ok(());
    }

    if version == 0 {
        let existing_tables: i64 = conn.query_row(
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

    let backup_path = if version > 0 && is_on_disk(db_path) {
        Some(create_pre_migration_backup(conn, db_path, version)?)
    } else {
        None
    };

    let start_version = version;
    for migration in migrations
        .iter()
        .filter(|migration| migration.target_version > start_version)
    {
        if migration.target_version != version + 1 {
            bail!(
                "non-contiguous schema migration from v{version} to v{}",
                migration.target_version
            );
        }
        let step_result: Result<()> = (|| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            (migration.apply)(&tx)?;
            verify_foreign_keys(&tx)?;
            verify_fts_integrity(&tx)?;
            tx.pragma_update(None, "user_version", migration.target_version)?;
            tx.commit()?;
            Ok(())
        })();

        if let Err(error) = step_result {
            if let Some(backup) = &backup_path {
                return Err(error).with_context(|| {
                    format!(
                        "schema migration to v{} ({}) failed; database rolled back and pre-migration backup is preserved at {}",
                        migration.target_version,
                        migration.name,
                        backup.display()
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
        version = migration.target_version;
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
) -> Result<PathBuf> {
    let backup_path = next_backup_path(db_path, from_version);
    let backup_str = backup_path.to_str().with_context(|| {
        format!(
            "pre-migration backup path is not valid UTF-8: {}",
            backup_path.display()
        )
    })?;
    conn.execute("VACUUM INTO ?1", [backup_str])
        .with_context(|| {
            format!(
                "failed to create pre-migration backup at {}",
                backup_path.display()
            )
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&backup_path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(backup_path)
}

fn next_backup_path(db_path: &Path, from_version: i64) -> PathBuf {
    let base = append_suffix(db_path, &format!(".v{from_version}.bak"));
    if !base.exists() {
        return base;
    }
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let unique = Uuid::now_v7().simple().to_string();
    append_suffix(
        db_path,
        &format!(".v{from_version}.{stamp}.{}.bak", &unique[..8]),
    )
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
        assert_eq!(version, 2);

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
        assert_eq!(version, 2);

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
}
