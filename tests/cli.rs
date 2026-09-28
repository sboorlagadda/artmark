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
