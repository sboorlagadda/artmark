use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use uuid::Uuid;

fn run(db: &Path, args: &[&str], input: Option<&str>) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artmark"));
    command.args(["--database", db.to_str().unwrap()]);
    command.args(args).arg("--json");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.is_empty(), "empty stdout; stderr: {stderr}");
    (
        output.status.code().unwrap(),
        serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("invalid JSON: {stdout}")),
    )
}

fn row_counts(db: &Path) -> (i64, i64, i64) {
    let conn = Connection::open(db).unwrap();
    let count = |table| {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    };
    (count("artifacts"), count("aliases"), count("artifact_fts"))
}

#[test]
fn credential_urls_leave_no_catalog_rows_or_secret_in_output() {
    let dir = std::env::temp_dir().join(format!("artmark-credentials-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");
    let secret = "secret-should-never-appear-in-error";
    for uri in [
        format!("https://example.com/file?token={secret}"),
        format!("https://example.com/file?X-Amz-Signature={secret}"),
        format!("https://example.com/file?sig={secret}"),
        format!("https://user:{secret}@example.com/file"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
            .args(["--database", db.to_str().unwrap(), "add", &uri, "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(6));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        assert_eq!(row_counts(&db), (0, 0, 0));
    }

    let (code, first) = run(&db, &["add", "https://example.com/item?id=1"], None);
    assert_eq!(code, 0);
    let (_, duplicate) = run(&db, &["add", "https://EXAMPLE.com/item?id=1#heading"], None);
    assert_eq!(duplicate["id"], first["id"]);
    let (_, distinct) = run(&db, &["add", "https://example.com/item?id=2"], None);
    assert_ne!(distinct["id"], first["id"]);
    assert_eq!(row_counts(&db), (2, 3, 2));

    let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "add",
            &format!("https://example.com/item?id=1&access_token={secret}"),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert_eq!(row_counts(&db), (2, 3, 2));

    let (code, _) = run(
        &db,
        &["add", "https://docs.google.com/document/d/ABC/edit"],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(row_counts(&db), (3, 4, 3));
    let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "add",
            &format!("https://docs.google.com/document/d/ABC/view?token={secret}"),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert_eq!(row_counts(&db), (3, 4, 3));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_registry_and_fts_work_end_to_end() {
    let dir = std::env::temp_dir().join(format!("artmark-cli-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");
    let (code, added) = run(
        &db,
        &[
            "add",
            "https://docs.google.com/document/d/ABC/edit",
            "--explicit",
        ],
        None,
    );
    assert_eq!(code, 0);
    let id = added["id"].as_str().unwrap();
    let (_, duplicate) = run(
        &db,
        &["add", "https://docs.google.com/document/d/ABC/view"],
        None,
    );
    assert_eq!(duplicate["id"], id);

    let entry = r#"{
      "source_metadata":{"title":"Enterprise Authentication","revision":"3"},
      "catalog":{"summary":"Enterprise identity migration", "search_text":"SAML migration with SCIM provisioning", "topics":["SAML"]},
      "retrieval":{"provider":"google_drive","external_id":"ABC"}
    }"#;
    let (code, indexed) = run(&db, &["index", id, "--json-input", "-"], Some(entry));
    assert_eq!(code, 0);
    assert_eq!(indexed["state"], "indexed");
    let (_, found) = run(&db, &["search", "SCIM", "--provider", "google_drive"], None);
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    assert_eq!(found["results"][0]["id"], id);
    let (_, reindexed) = run(&db, &["reindex"], None);
    assert_eq!(reindexed["reindexed"], 1);
    let (_, report) = run(&db, &["doctor"], None);
    assert_eq!(report["healthy"], true);
    let (_, forgotten) = run(&db, &["forget", id], None);
    assert_eq!(forgotten["forgotten"], true);
    let (_, empty) = run(&db, &["search", "SCIM"], None);
    assert_eq!(empty["results"].as_array().unwrap().len(), 0);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn default_database_uses_platform_home() {
    let home = std::env::temp_dir().join(format!("artmark-home-{}", Uuid::now_v7()));
    fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_artmark"));
    command.env_remove("ARTMARK_DB").env_remove("HOME");
    #[cfg(windows)]
    command.env("USERPROFILE", &home);
    #[cfg(not(windows))]
    command.env("HOME", &home);
    let output = command
        .args(["add", "https://example.com/fresh-install", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(home.join(".artmark/artmark.db").is_file());
    fs::remove_dir_all(home).unwrap();
}
