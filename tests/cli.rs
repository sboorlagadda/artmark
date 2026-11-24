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
        format!("//user:{secret}@example.com/file"),
        format!("https://example.com/file?next=%2Fdownload%3Ftoken%3D{secret}"),
        format!(
            "https://example.com/file?next=https%253A%252F%252Fexample.org%252Ffile%253Ftoken%253D{secret}"
        ),
        format!(
            "https://example.com/file#next=https%3A%2F%2Fexample.org%2Ffile%3Ftoken%3D{secret}"
        ),
        format!("https://example.com/file#https://user:{secret}@example.org/file"),
        format!("https://proxy.example/https://user:{secret}@example.org/file"),
        format!("https://proxy.example/https%3A%2F%2Fuser%3A{secret}%40example.org%2Ffile"),
        format!("archive/https://user:{secret}@example.org/file"),
        format!("https://proxy.example/https://user:{secret}@example.org/../../safe"),
        format!("https://example.com/file#access_token%253D{secret}"),
        format!("https://docs.google.com/document/d/ABC/edit?resourcekey={secret}"),
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

    let (code, nested) = run(
        &db,
        &[
            "add",
            "https://example.com/item?next=%2Fpage%3Fid%3D3&empty=",
        ],
        None,
    );
    assert_eq!(code, 0);
    assert_ne!(nested["id"], first["id"]);
    assert_eq!(row_counts(&db), (3, 4, 3));

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
    assert_eq!(row_counts(&db), (3, 4, 3));

    let (code, _) = run(
        &db,
        &["add", "https://docs.google.com/document/d/ABC/edit"],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(row_counts(&db), (4, 5, 4));
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
    assert_eq!(row_counts(&db), (4, 5, 4));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plain_credential_named_anchor_registers_as_an_alias() {
    let dir = std::env::temp_dir().join(format!("artmark-anchor-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");
    let (code, first) = run(&db, &["add", "https://example.com/docs"], None);
    assert_eq!(code, 0);
    let (code, second) = run(&db, &["add", "https://example.com/docs#code"], None);
    assert_eq!(code, 0);
    assert_eq!(second["id"], first["id"]);
    assert_eq!(row_counts(&db), (1, 2, 1));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn safe_double_slash_path_registers_as_filesystem_artifact() {
    let dir = std::env::temp_dir().join(format!("artmark-unc-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");
    let (code, added) = run(&db, &["add", "//localhost/artmark-missing/file"], None);
    assert_eq!(code, 0);
    let (_, artifact) = run(&db, &["get", added["id"].as_str().unwrap()], None);
    assert_eq!(artifact["provider"], "filesystem");
    assert_eq!(row_counts(&db), (1, 1, 1));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ordinary_code_query_registers_and_deduplicates() {
    let dir = std::env::temp_dir().join(format!("artmark-code-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");
    let (code, first) = run(
        &db,
        &["add", "https://example.com/products?code=ABC&state=CA"],
        None,
    );
    assert_eq!(code, 0);
    let (code, duplicate) = run(
        &db,
        &[
            "add",
            "https://EXAMPLE.com/products?code=ABC&state=CA#details",
        ],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(duplicate["id"], first["id"]);
    let (code, distinct) = run(
        &db,
        &["add", "https://example.com/products?code=XYZ&state=CA"],
        None,
    );
    assert_eq!(code, 0);
    assert_ne!(distinct["id"], first["id"]);
    assert_eq!(row_counts(&db), (2, 3, 2));
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
fn empty_search_explains_the_result_in_human_mode() {
    let dir = std::env::temp_dir().join(format!("artmark-empty-search-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args(["--database", db.to_str().unwrap(), "search", "unknown"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "No results found.\n"
    );
    assert!(output.stderr.is_empty());

    let (code, json) = run(&db, &["search", "unknown"], None);
    assert_eq!(code, 0);
    assert_eq!(json["results"].as_array().unwrap().len(), 0);
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

fn run_context_init(directory: &Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artmark"));
    command
        .args(["context", "init", "--json"])
        .current_dir(directory)
        .env_remove("ARTMARK_DB");
    #[cfg(windows)]
    command.env("USERPROFILE", directory);
    #[cfg(not(windows))]
    command.env("HOME", directory);
    command.output().unwrap()
}

fn run_doctor_with_codex_home(directory: &Path, codex_home: &Path) -> std::process::Output {
    let database = directory.join("doctor.db");
    let mut command = Command::new(env!("CARGO_BIN_EXE_artmark"));
    command
        .args(["--database", database.to_str().unwrap(), "doctor", "--json"])
        .current_dir(directory)
        .env("CODEX_HOME", codex_home)
        .env_remove("ARTMARK_DB");
    command.output().unwrap()
}

#[test]
fn doctor_reports_missing_and_installed_codex_skill() {
    let dir = std::env::temp_dir().join(format!("artmark-doctor-skill-{}", Uuid::now_v7()));
    let codex_home = dir.join("codex-home");
    let skill_path = codex_home.join("skills/artmark/SKILL.md");
    fs::create_dir_all(&dir).unwrap();

    let missing = run_doctor_with_codex_home(&dir, &codex_home);
    assert!(
        missing.status.success(),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    let missing_report: Value = serde_json::from_slice(&missing.stdout).unwrap();
    let missing_setup = &missing_report["agent_setup"];
    assert_eq!(missing_setup["codex_skill_installed"], false);
    assert_eq!(
        missing_setup["codex_skill_path"],
        skill_path.display().to_string()
    );
    let suggestions = missing_setup["suggestions"].as_array().unwrap();
    assert!(
        suggestions
            .iter()
            .any(|item| item.as_str().unwrap().contains("skill"))
    );
    assert!(
        suggestions
            .iter()
            .any(|item| item.as_str().unwrap().contains("artmark context init"))
    );

    fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
    fs::write(&skill_path, "---\nname: artmark\n---\n").unwrap();
    let existing = run_doctor_with_codex_home(&dir, &codex_home);
    assert!(
        existing.status.success(),
        "{}",
        String::from_utf8_lossy(&existing.stderr)
    );
    let existing_report: Value = serde_json::from_slice(&existing.stdout).unwrap();
    let existing_setup = &existing_report["agent_setup"];
    assert_eq!(existing_setup["codex_skill_installed"], true);
    assert_eq!(
        existing_setup["codex_skill_path"],
        skill_path.display().to_string()
    );
    assert_eq!(existing_setup["suggestions"].as_array().unwrap().len(), 0);

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn context_init_creates_and_updates_idempotently_and_rejects_bad_markers() {
    let dir = std::env::temp_dir().join(format!("artmark-context-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();

    let fresh_dir = dir.join("fresh-project");
    fs::create_dir(&fresh_dir).unwrap();
    let fresh_first = run_context_init(&fresh_dir);
    assert!(
        fresh_first.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh_first.stderr)
    );
    let fresh_first_result: Value = serde_json::from_slice(&fresh_first.stdout).unwrap();
    assert_eq!(fresh_first_result["action"], "created");
    let fresh_agents = fs::read_to_string(fresh_dir.join("AGENTS.md")).unwrap();
    let fresh_second = run_context_init(&fresh_dir);
    assert!(
        fresh_second.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh_second.stderr)
    );
    let fresh_second_result: Value = serde_json::from_slice(&fresh_second.stdout).unwrap();
    assert_eq!(fresh_second_result["action"], "unchanged");
    assert_eq!(
        fs::read_to_string(fresh_dir.join("AGENTS.md")).unwrap(),
        fresh_agents
    );
    assert!(!fresh_dir.join(".artmark/artmark.db").exists());

    let agents = dir.join("AGENTS.md");
    let prefix = "# Project\r\n\r\nKeep this before the generated section.\r\n\r\n";
    let old_section = "<!-- artmark:context:start -->\r\nOld artmark instructions.\r\n<!-- artmark:context:end -->";
    let suffix = "\r\n\r\nKeep this after the generated section.\r\n";
    fs::write(&agents, format!("{prefix}{old_section}{suffix}")).unwrap();

    let first = run_context_init(&dir);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_result: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_result["action"], "updated");
    let generated = fs::read_to_string(&agents).unwrap();
    assert!(generated.starts_with(prefix));
    assert!(generated.ends_with(suffix));
    assert!(generated.contains("## Using artmark"));
    assert!(generated.contains("Do not automatically register"));
    assert!(generated.contains("--json-input"));
    assert!(
        !generated.replace("\r\n", "").contains('\n'),
        "context init should preserve CRLF throughout the file"
    );

    let second = run_context_init(&dir);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_result: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second_result["action"], "unchanged");
    assert_eq!(fs::read_to_string(&agents).unwrap(), generated);
    assert!(!dir.join(".artmark/artmark.db").exists());

    let malformed_cases = [
        (
            "incomplete",
            "# Project\n<!-- artmark:context:start -->\nIncomplete section\n",
        ),
        (
            "duplicated",
            "<!-- artmark:context:start -->\nfirst\n<!-- artmark:context:end -->\n<!-- artmark:context:start -->\nsecond\n<!-- artmark:context:end -->\n",
        ),
        (
            "malformed",
            "# Project <!-- artmark:context:start -->\nOld section\n<!-- artmark:context:end -->\n",
        ),
        (
            "reversed",
            "<!-- artmark:context:end -->\nOld section\n<!-- artmark:context:start -->\n",
        ),
    ];
    for (name, contents) in malformed_cases {
        let case_dir = dir.join(name);
        fs::create_dir(&case_dir).unwrap();
        let case_agents = case_dir.join("AGENTS.md");
        fs::write(&case_agents, contents).unwrap();
        let output = run_context_init(&case_dir);
        assert_eq!(output.status.code(), Some(6), "case: {name}");
        assert!(output.stdout.is_empty(), "case: {name}");
        assert_eq!(
            fs::read_to_string(&case_agents).unwrap(),
            contents,
            "case: {name}"
        );
    }

    fs::remove_dir_all(dir).unwrap();
}

// macOS rejects non-UTF-8 filenames before artmark can inspect the path.
#[cfg(target_os = "linux")]
#[test]
fn context_init_json_handles_non_utf8_current_directory() {
    use std::os::unix::ffi::OsStringExt;

    let mut name = format!("artmark-nonutf8-{}", Uuid::now_v7()).into_bytes();
    name.push(0xff);
    let dir = std::env::temp_dir().join(std::ffi::OsString::from_vec(name));
    fs::create_dir(&dir).unwrap();

    let output = run_context_init(&dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["action"], "created");
    assert!(result["path"].as_str().unwrap().contains('\u{fffd}'));
    assert!(dir.join("AGENTS.md").is_file());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn add_with_json_input_indexes_atomically_and_rejects_invalid_cards() {
    let dir = std::env::temp_dir().join(format!("artmark-add-index-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let bad_card =
        r#"{"source_metadata":{"body":"nope"},"catalog":{"summary":"x","search_text":"y"}}"#;
    let mut bad_cmd = Command::new(env!("CARGO_BIN_EXE_artmark"));
    bad_cmd
        .args([
            "--database",
            db.to_str().unwrap(),
            "add",
            "https://docs.google.com/document/d/ONE/edit",
            "--explicit",
            "--json-input",
            "-",
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = bad_cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(bad_card.as_bytes())
        .unwrap();
    let bad_output = child.wait_with_output().unwrap();
    assert_eq!(bad_output.status.code(), Some(6));
    assert!(bad_output.stdout.is_empty());
    assert_eq!(row_counts(&db), (0, 0, 0));

    let card = r#"{
      "source_metadata":{"title":"Webhook Retries RFC","revision":"2"},
      "catalog":{
        "summary":"Retry backoff and dead-letter queue design",
        "search_text":"webhook retries exponential backoff idempotency key dlq",
        "topics":["webhooks","reliability"]
      },
      "retrieval":{"provider":"google_drive","external_id":"ONE"}
    }"#;
    let (code, added) = run(
        &db,
        &[
            "add",
            "https://docs.google.com/document/d/ONE/edit",
            "--explicit",
            "--json-input",
            "-",
        ],
        Some(card),
    );
    assert_eq!(code, 0);
    assert_eq!(added["state"], "indexed");
    assert_eq!(added["is_explicitly_saved"], true);
    assert_eq!(added["title"], "Webhook Retries RFC");
    let id = added["id"].as_str().unwrap();

    let (code, found) = run(&db, &["search", "idempotency"], None);
    assert_eq!(code, 0);
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    assert_eq!(found["results"][0]["id"], id);

    let card_file = dir.join("card.json");
    fs::write(
        &card_file,
        r#"{"catalog":{"summary":"Updated retry design","search_text":"webhook circuit breaker"}}"#,
    )
    .unwrap();
    let human = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "add",
            "https://docs.google.com/document/d/ONE/view",
            "--json-input",
            card_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(human.status.success());
    let human_stdout = String::from_utf8(human.stdout).unwrap();
    let lines: Vec<_> = human_stdout.lines().collect();
    assert_eq!(lines, vec![id, "gdrive:doc:ONE", "indexed"]);
    assert_eq!(row_counts(&db), (1, 2, 1));

    let secret = "top-secret-token";
    let cred = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "add",
            &format!("https://docs.google.com/document/d/ONE/preview?token={secret}"),
            "--json-input",
            card_file.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(cred.status.code(), Some(6));
    assert!(cred.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&cred.stderr).contains(secret));
    assert_eq!(row_counts(&db), (1, 2, 1));

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recent_filters_by_state_and_provider() {
    let dir = std::env::temp_dir().join(format!("artmark-recent-filter-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let (_, indexed) = run(
        &db,
        &[
            "add",
            "https://docs.google.com/document/d/IDX/edit",
            "--explicit",
            "--json-input",
            "-",
        ],
        Some(
            r#"{"source_metadata":{"title":"Indexed Doc"},"catalog":{"summary":"Doc","search_text":"indexed doc"}}"#,
        ),
    );
    let (_, pointer_only) = run(
        &db,
        &[
            "add",
            "https://github.com/sboorlagadda/artmark/issues/28",
            "--explicit",
        ],
        None,
    );

    let (code, reg) = run(&db, &["recent", "--state", "registered"], None);
    assert_eq!(code, 0);
    assert_eq!(reg["results"].as_array().unwrap().len(), 1);
    assert_eq!(reg["results"][0]["id"], pointer_only["id"]);

    let (code, idx) = run(&db, &["recent", "--state", "indexed"], None);
    assert_eq!(code, 0);
    assert_eq!(idx["results"].as_array().unwrap().len(), 1);
    assert_eq!(idx["results"][0]["id"], indexed["id"]);

    let (code, gh) = run(
        &db,
        &["recent", "--state", "registered", "--provider", "github"],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(gh["results"].as_array().unwrap().len(), 1);
    assert_eq!(gh["results"][0]["id"], pointer_only["id"]);

    let empty_human = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args([
            "--database",
            db.to_str().unwrap(),
            "recent",
            "--state",
            "unreachable",
        ])
        .output()
        .unwrap();
    assert!(empty_human.status.success());
    assert_eq!(
        String::from_utf8(empty_human.stdout).unwrap(),
        "No results found.\n"
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_upgrades_v1_database_creates_backup_and_refuses_newer_schema() {
    let dir = std::env::temp_dir().join(format!("artmark-v1-upgrade-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
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
             INSERT INTO artifacts (
               id, canonical_key, kind, provider, state, primary_uri, title, summary, search_text,
               source_external_id, is_explicitly_saved, created_at, updated_at, last_seen_at, last_indexed_at
             ) VALUES (
               'art_legacy_1',
               'gdrive:doc:MIGRATE1',
               'google_doc',
               'google_drive',
               'indexed',
               'https://docs.google.com/document/d/MIGRATE1/edit',
               'Enterprise Identity Migration',
               'Plan for identity migration and webhook retries',
               'SAML migration identity provisioning webhook retries',
               'MIGRATE1',
               1,
               '2026-06-01T00:00:00.000Z',
               '2026-06-01T00:00:00.000Z',
               '2026-06-01T00:00:00.000Z',
               '2026-06-01T00:00:00.000Z'
             );
             INSERT INTO aliases VALUES ('https://docs.google.com/document/d/MIGRATE1/edit', 'art_legacy_1', '2026-06-01T00:00:00.000Z', '2026-06-01T00:00:00.000Z');
             INSERT INTO topics VALUES ('art_legacy_1', 'SAML');
             INSERT INTO artifact_fts VALUES (
               'art_legacy_1',
               'Enterprise Identity Migration',
               'Plan for identity migration and webhook retries',
               'SAML migration identity provisioning webhook retries',
               'SAML', '', ''
             );
             PRAGMA user_version=1;",
        )
        .unwrap();
    }

    let (code, doctor) = run(&db, &["doctor"], None);
    assert_eq!(code, 0);
    assert_eq!(doctor["healthy"], true);
    assert_eq!(doctor["schema_version"], 2);
    assert_eq!(doctor["foreign_key_integrity"], true);
    assert_eq!(doctor["fts_integrity"], true);

    let backup = dir.join("artmark.db.v1.bak");
    assert!(backup.is_file(), "expected pre-migration backup {backup:?}");

    let (code, found) = run(&db, &["search", "migrating identities"], None);
    assert_eq!(code, 0);
    assert_eq!(found["results"].as_array().unwrap().len(), 1);
    assert_eq!(found["results"][0]["id"], "art_legacy_1");

    let (code, item) = run(&db, &["get", "art_legacy_1", "--no-track"], None);
    assert_eq!(code, 0);
    assert_eq!(item["access_count"], 0);
    assert!(item["last_accessed_at"].is_null());

    // Refuse opening a database from a newer unsupported schema version.
    let future_db = dir.join("future.db");
    {
        let conn = Connection::open(&future_db).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
        .args(["--database", future_db.to_str().unwrap(), "stats", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("newer than this artmark build"));
    assert!(!dir.join("future.db.v99.bak").exists());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_tracks_get_lookups_honors_no_track_and_reports_usage_in_stats() {
    let dir = std::env::temp_dir().join(format!("artmark-lookup-stats-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let (_, first) = run(
        &db,
        &[
            "add",
            "https://docs.google.com/document/d/USAGE1/edit",
            "--explicit",
            "--json-input",
            "-",
        ],
        Some(
            r#"{"source_metadata":{"title":"Anchor Spec"},"catalog":{"summary":"Anchor spec","search_text":"anchor spec webhook retries"}}"#,
        ),
    );
    let (_, second) = run(
        &db,
        &[
            "add",
            "https://github.com/org/repo/issues/12",
            "--explicit",
            "--json-input",
            "-",
        ],
        Some(
            r#"{"source_metadata":{"title":"Retry Bug"},"catalog":{"summary":"Bug","search_text":"webhook retries storm"}}"#,
        ),
    );
    let id1 = first["id"].as_str().unwrap();
    let id2 = second["id"].as_str().unwrap();

    // Search, recent, and --no-track do not increment access counts.
    let _ = run(&db, &["search", "webhook retries"], None);
    let _ = run(&db, &["recent"], None);
    let (_, peek) = run(&db, &["get", id1, "--no-track"], None);
    assert_eq!(peek["access_count"], 0);
    assert!(peek["last_accessed_at"].is_null());

    // Tracked gets increment access_count and populate last_accessed_at.
    let (_, g1) = run(&db, &["get", id1], None);
    assert_eq!(g1["access_count"], 1);
    assert!(g1["last_accessed_at"].is_string());
    let (_, g2) = run(
        &db,
        &["get", "https://docs.google.com/document/d/USAGE1/view"],
        None,
    );
    assert_eq!(g2["access_count"], 2);
    let (_, g3) = run(&db, &["get", id2], None);
    assert_eq!(g3["access_count"], 1);

    // Stats surfaces local usage summaries.
    let (code, stats) = run(&db, &["stats"], None);
    assert_eq!(code, 0);
    assert_eq!(stats["total_accesses"], 3);
    assert_eq!(stats["accessed_artifacts"], 2);
    let top = stats["top_accessed"].as_array().unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0]["id"], id1);
    assert_eq!(top[0]["access_count"], 2);
    assert_eq!(top[1]["id"], id2);
    assert_eq!(top[1]["access_count"], 1);

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn search_evaluation_corpus_achieves_target_retrieval_quality() {
    let dir = std::env::temp_dir().join(format!("artmark-search-eval-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let corpus_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/search_eval.json");
    let corpus: Value = serde_json::from_str(&fs::read_to_string(corpus_path).unwrap()).unwrap();

    let mut id_to_label = std::collections::HashMap::new();
    for artifact in corpus["artifacts"].as_array().unwrap() {
        let label = artifact["label"].as_str().unwrap().to_owned();
        let uri = artifact["uri"].as_str().unwrap();
        let mut args = vec!["add", uri];
        if artifact["explicit"].as_bool() == Some(true) {
            args.push("--explicit");
        }
        if let Some(title) = artifact["title"].as_str() {
            args.push("--title");
            args.push(title);
        }
        let card_str;
        let stdin = if artifact["card"].is_null() {
            None
        } else {
            args.push("--json-input");
            args.push("-");
            card_str = serde_json::to_string(&artifact["card"]).unwrap();
            Some(card_str.as_str())
        };
        let (code, created) = run(&db, &args, stdin);
        assert_eq!(code, 0, "failed to add {label}");
        id_to_label.insert(created["id"].as_str().unwrap().to_owned(), label);

        if let Some(aliases) = artifact.get("aliases").and_then(Value::as_array) {
            for alias in aliases {
                let (alias_code, _) = run(&db, &["add", alias.as_str().unwrap()], None);
                assert_eq!(alias_code, 0);
            }
        }
    }

    let mut evaluated = 0usize;
    let mut hit_at_1 = 0usize;
    for case in corpus["queries"].as_array().unwrap() {
        let qid = case["id"].as_str().unwrap();
        let query = case["query"].as_str().unwrap();
        let expected = case["expected_top1"].as_str().unwrap();
        let is_boundary = case
            .get("expected_lexical_miss")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut args = vec![
            "search".to_owned(),
            query.to_owned(),
            "--limit".to_owned(),
            "5".to_owned(),
        ];
        if let Some(filters) = case.get("filters").and_then(Value::as_object) {
            for (key, value) in filters {
                args.push(format!("--{}", key.replace('_', "-")));
                args.push(value.as_str().unwrap().to_owned());
            }
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, response) = run(&db, &arg_refs, None);
        assert_eq!(code, 0, "query failed: {qid}");
        let results = response["results"].as_array().unwrap();
        let returned: Vec<&str> = results
            .iter()
            .map(|row| id_to_label[row["id"].as_str().unwrap()].as_str())
            .collect();

        if is_boundary {
            assert!(
                returned.first().copied() != Some(expected),
                "expected lexical boundary miss for {qid}, got {returned:?}"
            );
            continue;
        }

        evaluated += 1;
        assert_eq!(
            returned.first().copied(),
            Some(expected),
            "query {qid} ({query:?}) expected top1={expected}, got {returned:?}"
        );
        hit_at_1 += 1;
    }

    assert_eq!(evaluated, 28);
    assert_eq!(hit_at_1, 28);

    fs::remove_dir_all(dir).unwrap();
}
