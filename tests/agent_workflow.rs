//! End-to-end cross-session agent workflow tests with an external mock provider (Issue #9).
//!
//! Architecture boundary verified here:
//! - `MockProvider` lives completely outside `artmark` and holds authoritative live documents,
//!   revisions, full document bodies, access controls, and provider bearer credentials.
//! - Each `AgentSession` starts fresh with zero in-memory memory of previous sessions; the only
//!   shared durable state between sessions is the SQLite database on disk and the external provider.
//! - Every CLI invocation runs the compiled `artmark` binary as a fresh subprocess.

use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const PROVIDER_SECRET: &str = "MOCK_PROVIDER_BEARER_TOKEN_DO_NOT_PERSIST_999";
const ANCHOR_BODY_REV1_SENTINEL: &str = "FULL_SOURCE_BODY_SENTINEL_REV1_SECRET_EXCERPT";
const ANCHOR_BODY_REV2_SENTINEL: &str = "FULL_SOURCE_BODY_SENTINEL_REV2_CIRCUIT_BREAKER_TABLE";
const JIRA_BODY_SENTINEL: &str = "JIRA_FULL_TICKET_TRANSCRIPT_SENTINEL_DO_NOT_STORE";

#[derive(Debug, Clone)]
struct LiveDocument {
    title: String,
    revision: String,
    updated_at: String,
    summary_hint: String,
    retrieval_terms: String,
    topics: Vec<String>,
    entities: Vec<String>,
    tags: Vec<String>,
    live_detail: String,
    full_body: String,
    accessible: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderError {
    Unauthorized,
    AccessDenied,
    NotFound,
}

/// Simulates external provider tools (Google Drive, Jira, etc.) owned by the agent harness.
struct MockProvider {
    expected_token: String,
    docs: HashMap<(String, String), LiveDocument>,
    uri_index: HashMap<String, (String, String)>,
}

impl MockProvider {
    fn new(expected_token: &str) -> Self {
        Self {
            expected_token: expected_token.to_owned(),
            docs: HashMap::new(),
            uri_index: HashMap::new(),
        }
    }

    fn insert(&mut self, provider: &str, external_id: &str, uris: &[&str], doc: LiveDocument) {
        let key = (provider.to_owned(), external_id.to_owned());
        self.docs.insert(key.clone(), doc);
        for uri in uris {
            self.uri_index.insert((*uri).to_owned(), key.clone());
        }
    }

    fn fetch_by_uri(
        &self,
        token: &str,
        uri: &str,
    ) -> Result<((String, String), LiveDocument), ProviderError> {
        if token != self.expected_token {
            return Err(ProviderError::Unauthorized);
        }
        let key = self.uri_index.get(uri).ok_or(ProviderError::NotFound)?;
        let doc = self.docs.get(key).ok_or(ProviderError::NotFound)?;
        if !doc.accessible {
            return Err(ProviderError::AccessDenied);
        }
        Ok((key.clone(), doc.clone()))
    }

    fn fetch_by_hint(
        &self,
        token: &str,
        provider: &str,
        external_id: &str,
    ) -> Result<LiveDocument, ProviderError> {
        if token != self.expected_token {
            return Err(ProviderError::Unauthorized);
        }
        let doc = self
            .docs
            .get(&(provider.to_owned(), external_id.to_owned()))
            .ok_or(ProviderError::NotFound)?;
        if !doc.accessible {
            return Err(ProviderError::AccessDenied);
        }
        Ok(doc.clone())
    }

    fn update_doc(&mut self, provider: &str, external_id: &str, doc: LiveDocument) {
        self.docs
            .insert((provider.to_owned(), external_id.to_owned()), doc);
    }

    fn set_accessible(&mut self, provider: &str, external_id: &str, accessible: bool) {
        if let Some(doc) = self
            .docs
            .get_mut(&(provider.to_owned(), external_id.to_owned()))
        {
            doc.accessible = accessible;
        }
    }
}

/// Stateless simulated agent session: communicates only with the `artmark` CLI and external harness tools.
struct AgentSession<'a> {
    db: PathBuf,
    provider: &'a MockProvider,
    provider_token: &'a str,
    version_checked: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct SaveOutcome {
    id: String,
    state: String,
    searchable_by_topic: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum RediscoveryOutcome {
    AnsweredFromLiveSource {
        id: String,
        canonical_key: String,
        live_detail: String,
        revision: String,
        refreshed_card: bool,
    },
    PointerFoundButSourceUnavailable {
        id: String,
        canonical_key: String,
        primary_uri: String,
        card_was_not_treated_as_live_truth: bool,
    },
    NotFound,
}

impl<'a> AgentSession<'a> {
    fn new(db: &Path, provider: &'a MockProvider, provider_token: &'a str) -> Self {
        Self {
            db: db.to_path_buf(),
            provider,
            provider_token,
            version_checked: false,
        }
    }

    fn ensure_cli_available(&mut self) {
        if self.version_checked {
            return;
        }
        let output = Command::new(env!("CARGO_BIN_EXE_artmark"))
            .arg("--version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .starts_with("artmark ")
        );
        self.version_checked = true;
    }

    fn run_cli(&mut self, args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
        self.ensure_cli_available();
        let mut command = Command::new(env!("CARGO_BIN_EXE_artmark"));
        command.args(["--database", self.db.to_str().unwrap()]);
        command.args(args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().unwrap();
        if let Some(payload) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(payload.as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        (
            output.status.code().unwrap(),
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }

    fn run_json(&mut self, args: &[&str], stdin: Option<&str>) -> Value {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let (code, stdout, stderr) = self.run_cli(&with_json, stdin);
        assert_eq!(code, 0, "artmark {args:?} failed; stderr: {stderr}");
        assert!(!stdout.is_empty(), "empty stdout; stderr: {stderr}");
        serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("invalid JSON: {stdout}"))
    }

    /// Follows SKILL.md: explicit save inspects source through harness tool and writes pointer +
    /// card atomically; falls back to pointer-only only if the source cannot be inspected.
    fn explicit_save(&mut self, uri: &str, fallback_title: Option<&str>) -> SaveOutcome {
        match self.provider.fetch_by_uri(self.provider_token, uri) {
            Ok(((provider_name, external_id), doc)) => {
                // Harness reads the live body to synthesize a compact retrieval card, never
                // copying the body itself into artmark.
                assert!(!doc.full_body.is_empty());
                let card = build_search_card(&provider_name, &external_id, &doc);
                let payload = serde_json::to_string(&card).unwrap();
                let saved = self.run_json(
                    &["add", uri, "--explicit", "--json-input", "-"],
                    Some(&payload),
                );
                SaveOutcome {
                    id: saved["id"].as_str().unwrap().to_owned(),
                    state: saved["state"].as_str().unwrap().to_owned(),
                    searchable_by_topic: true,
                }
            }
            Err(_) => {
                let mut args = vec!["add", uri, "--explicit"];
                if let Some(title) = fallback_title {
                    args.push("--title");
                    args.push(title);
                }
                let saved = self.run_json(&args, None);
                SaveOutcome {
                    id: saved["id"].as_str().unwrap().to_owned(),
                    state: saved["state"].as_str().unwrap().to_owned(),
                    searchable_by_topic: false,
                }
            }
        }
    }

    /// Follows SKILL.md: search artmark first, inspect match with `artmark get`, retrieve live
    /// source from the harness provider, and opportunistically refresh the card on material drift.
    fn rediscover_and_answer(&mut self, vague_query: &str) -> RediscoveryOutcome {
        let search = self.run_json(&["search", vague_query], None);
        let results = search["results"].as_array().unwrap();
        let Some(top) = results.first() else {
            return RediscoveryOutcome::NotFound;
        };
        let id = top["id"].as_str().unwrap().to_owned();

        // Explicit tracked lookup increments access_count and fetches full retrieval hints.
        let entry = self.run_json(&["get", &id], None);
        let canonical_key = entry["canonical_key"].as_str().unwrap().to_owned();
        let primary_uri = entry["primary_uri"].as_str().unwrap().to_owned();
        let provider_name = entry["retrieval"]["provider"]
            .as_str()
            .or_else(|| entry["provider"].as_str())
            .unwrap()
            .to_owned();
        let external_id = entry["retrieval"]["external_id"]
            .as_str()
            .or_else(|| entry["source_external_id"].as_str())
            .unwrap()
            .to_owned();
        let indexed_revision = entry["source_revision"].as_str().map(str::to_owned);

        match self
            .provider
            .fetch_by_hint(self.provider_token, &provider_name, &external_id)
        {
            Ok(live_doc) => {
                let mut refreshed_card = false;
                if indexed_revision.as_deref() != Some(live_doc.revision.as_str()) {
                    let card = build_search_card(&provider_name, &external_id, &live_doc);
                    let payload = serde_json::to_string(&card).unwrap();
                    let updated =
                        self.run_json(&["index", &id, "--json-input", "-"], Some(&payload));
                    assert_eq!(updated["state"], "indexed");
                    refreshed_card = true;
                }
                RediscoveryOutcome::AnsweredFromLiveSource {
                    id,
                    canonical_key,
                    live_detail: live_doc.live_detail,
                    revision: live_doc.revision,
                    refreshed_card,
                }
            }
            Err(
                ProviderError::AccessDenied | ProviderError::Unauthorized | ProviderError::NotFound,
            ) => RediscoveryOutcome::PointerFoundButSourceUnavailable {
                id,
                canonical_key,
                primary_uri,
                card_was_not_treated_as_live_truth: true,
            },
        }
    }
}

/// Builds a compact, retrieval-oriented search card from inspected metadata without copying
/// source bodies or credentials.
fn build_search_card(provider: &str, external_id: &str, doc: &LiveDocument) -> Value {
    json!({
        "source_metadata": {
            "title": doc.title,
            "revision": doc.revision,
            "updated_at": doc.updated_at
        },
        "catalog": {
            "summary": doc.summary_hint,
            "search_text": doc.retrieval_terms,
            "topics": doc.topics,
            "entities": doc.entities,
            "tags": doc.tags
        },
        "retrieval": {
            "provider": provider,
            "external_id": external_id
        },
        "resolver": "mock_harness_v1"
    })
}

fn assert_sqlite_contains_no_forbidden_strings(db_path: &Path, forbidden: &[&str]) {
    let conn = Connection::open(db_path).unwrap();
    for table in [
        "artifacts",
        "aliases",
        "topics",
        "entities",
        "tags",
        "artifact_fts",
    ] {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |row| {
                let mut texts = Vec::new();
                for idx in 0..columns {
                    if let Ok(val) = row.get::<_, String>(idx) {
                        texts.push(val);
                    }
                }
                Ok(texts)
            })
            .unwrap();
        for row in rows {
            for cell in row.unwrap() {
                for needle in forbidden {
                    assert!(
                        !cell.contains(needle),
                        "forbidden string {needle:?} leaked into SQLite table {table}: {cell}"
                    );
                }
            }
        }
    }
}

#[test]
fn cross_session_workflow_saves_rediscovers_refreshes_and_handles_access_denied() {
    let dir = std::env::temp_dir().join(format!("artmark-workflow-{}", Uuid::now_v7()));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("artmark.db");

    let doc_edit_uri = "https://docs.google.com/document/u/0/d/WEBHOOK_ANCHOR/edit";
    let doc_view_uri =
        "https://docs.google.com/document/d/WEBHOOK_ANCHOR/view?usp=sharing#heading=h.1";
    let jira_browse_uri = "https://acme.atlassian.net/browse/ENG-123";
    let jira_board_uri =
        "https://acme.atlassian.net/jira/software/projects/ENG/boards/4?selectedIssue=ENG-123";

    let mut provider = MockProvider::new(PROVIDER_SECRET);
    provider.insert(
        "google_drive",
        "WEBHOOK_ANCHOR",
        &[doc_edit_uri, doc_view_uri],
        LiveDocument {
            title: "Webhook Retries Framing Doc".into(),
            revision: "1".into(),
            updated_at: "2026-10-01T10:00:00Z".into(),
            summary_hint: "Framing doc for outbound webhook retry backoff and delivery guarantees."
                .into(),
            retrieval_terms:
                "framing doc outbound webhook retries exponential backoff jitter idempotency keys"
                    .into(),
            topics: vec!["webhooks".into(), "reliability".into()],
            entities: vec!["Event Bus".into()],
            tags: vec!["framing".into(), "rfc".into()],
            live_detail: "Current live policy: max_attempts=7 with full jitter and 30s cap.".into(),
            full_body: format!(
                "Long authoritative specification...\n{ANCHOR_BODY_REV1_SENTINEL}\nDetails..."
            ),
            accessible: true,
        },
    );
    provider.insert(
        "jira",
        "ENG-123",
        &[jira_browse_uri, jira_board_uri],
        LiveDocument {
            title: "Mitigate partner 502 webhook retry storms".into(),
            revision: "1".into(),
            updated_at: "2026-10-05T12:00:00Z".into(),
            summary_hint: "Jira ticket tracking partner 502 outage retry storms.".into(),
            retrieval_terms: "jira ticket partner 502 thundering herd webhook storm mitigation"
                .into(),
            topics: vec!["webhooks".into(), "incidents".into()],
            entities: vec!["ENG-123".into()],
            tags: vec!["bug".into()],
            live_detail: "Live status: In Progress, assigned to Platform SRE.".into(),
            full_body: format!("Private ticket discussion...\n{JIRA_BODY_SENTINEL}"),
            // Initially inaccessible to test pointer-only fallback.
            accessible: false,
        },
    );

    // -------------------------------------------------------------------------
    // Session 1: Explicit save + pointer-only fallback + deduplication + secret rejection.
    // -------------------------------------------------------------------------
    let (anchor_id, jira_id) = {
        let mut session1 = AgentSession::new(&db, &provider, PROVIDER_SECRET);

        // Policy check: unrequested pasted links are NOT registered automatically.
        let initial_stats = session1.run_json(&["stats"], None);
        assert_eq!(initial_stats["artifacts"], 0);

        // Explicit save of readable anchor doc -> atomic inspect + index.
        let saved_doc = session1.explicit_save(doc_edit_uri, None);
        assert_eq!(saved_doc.state, "indexed");
        assert!(saved_doc.searchable_by_topic);

        // Explicit save of currently inaccessible Jira ticket -> pointer-only fallback.
        let saved_jira = session1.explicit_save(jira_browse_uri, Some("ENG-123"));
        assert_eq!(saved_jira.state, "registered");
        assert!(!saved_jira.searchable_by_topic);

        // Pointer-only entry is discoverable for later indexing via `recent --state registered`.
        let pending = session1.run_json(&["recent", "--state", "registered"], None);
        let pending_rows = pending["results"].as_array().unwrap();
        assert_eq!(pending_rows.len(), 1);
        assert_eq!(pending_rows[0]["id"], saved_jira.id);

        // Alternate URL variant of the same Google Doc deduplicates onto the exact same record.
        let dedup_doc = session1.explicit_save(doc_view_uri, None);
        assert_eq!(dedup_doc.id, saved_doc.id);
        assert_eq!(dedup_doc.state, "indexed");

        // Credential-bearing URL from provider is rejected with exit 6 and redacted stderr.
        let signed_uri = format!("{doc_view_uri}&token={PROVIDER_SECRET}");
        let (code, stdout, stderr) = session1.run_cli(&["add", &signed_uri, "--json"], None);
        assert_eq!(code, 6);
        assert!(stdout.is_empty());
        assert!(!stderr.contains(PROVIDER_SECRET));

        // Accidentally passing source body or provider token in source_metadata is rejected.
        let leaky_card = serde_json::to_string(&json!({
            "source_metadata": {
                "title": "Leaky Card",
                "body": ANCHOR_BODY_REV1_SENTINEL,
                "access_token": PROVIDER_SECRET
            },
            "catalog": {
                "summary": "Should be rejected",
                "search_text": "should never persist"
            }
        }))
        .unwrap();
        let (code, stdout, _) = session1.run_cli(
            &["index", &saved_doc.id, "--json-input", "-", "--json"],
            Some(&leaky_card),
        );
        assert_eq!(code, 6);
        assert!(stdout.is_empty());

        (saved_doc.id, saved_jira.id)
    };

    // -------------------------------------------------------------------------
    // Session 2 (fresh session, zero in-memory state): Vague query -> live source retrieval.
    // -------------------------------------------------------------------------
    {
        let mut session2 = AgentSession::new(&db, &provider, PROVIDER_SECRET);
        let outcome =
            session2.rediscover_and_answer("where is my framing doc for webhook retry backoff");
        assert_eq!(
            outcome,
            RediscoveryOutcome::AnsweredFromLiveSource {
                id: anchor_id.clone(),
                canonical_key: "gdrive:doc:WEBHOOK_ANCHOR".into(),
                live_detail: "Current live policy: max_attempts=7 with full jitter and 30s cap."
                    .into(),
                revision: "1".into(),
                refreshed_card: false,
            }
        );

        // Confirm explicit `get` tracked the rediscovery lookup locally.
        let stats = session2.run_json(&["stats"], None);
        assert_eq!(stats["artifacts"], 2);
        assert_eq!(stats["total_accesses"], 1);
        assert_eq!(stats["accessed_artifacts"], 1);
        assert_eq!(stats["top_accessed"][0]["id"], anchor_id);
    }

    // -------------------------------------------------------------------------
    // Session 3 (fresh session): Live source changes -> opportunistic card refresh + Jira indexing.
    // -------------------------------------------------------------------------
    provider.update_doc(
        "google_drive",
        "WEBHOOK_ANCHOR",
        LiveDocument {
            title: "Webhook Retries and Circuit Breaker Framing Doc".into(),
            revision: "2".into(),
            updated_at: "2026-10-18T14:30:00Z".into(),
            summary_hint:
                "Framing doc for outbound webhook retries, dead-letter queue replay, and circuit breakers."
                    .into(),
            retrieval_terms:
                "framing doc outbound webhook retries exponential backoff dead letter queue dlq replay circuit breaker"
                    .into(),
            topics: vec![
                "webhooks".into(),
                "reliability".into(),
                "circuit-breakers".into(),
            ],
            entities: vec!["Event Bus".into()],
            tags: vec!["framing".into(), "rfc".into()],
            live_detail:
                "Current live policy (rev 2): open circuit breaker after 50% 5xx over 60s and replay from DLQ."
                    .into(),
            full_body: format!(
                "Updated authoritative spec rev 2...\n{ANCHOR_BODY_REV2_SENTINEL}\nDetails..."
            ),
            accessible: true,
        },
    );
    provider.set_accessible("jira", "ENG-123", true);

    {
        let mut session3 = AgentSession::new(&db, &provider, PROVIDER_SECRET);
        let outcome = session3.rediscover_and_answer("webhook retry framing doc");
        assert_eq!(
            outcome,
            RediscoveryOutcome::AnsweredFromLiveSource {
                id: anchor_id.clone(),
                canonical_key: "gdrive:doc:WEBHOOK_ANCHOR".into(),
                live_detail:
                    "Current live policy (rev 2): open circuit breaker after 50% 5xx over 60s and replay from DLQ."
                        .into(),
                revision: "2".into(),
                refreshed_card: true,
            }
        );

        // Verify refresh preserved ID, aliases, explicit flag, and incremented access_count.
        let refreshed = session3.run_json(&["get", &anchor_id, "--no-track"], None);
        assert_eq!(refreshed["source_revision"], "2");
        assert_eq!(refreshed["is_explicitly_saved"], true);
        assert_eq!(refreshed["access_count"], 2);
        assert_eq!(refreshed["aliases"].as_array().unwrap().len(), 2);

        // Jira ticket is now inspectable via an alternate board URL -> deduplicates and indexes.
        let indexed_jira = session3.explicit_save(jira_board_uri, None);
        assert_eq!(indexed_jira.id, jira_id);
        assert_eq!(indexed_jira.state, "indexed");
    }

    // -------------------------------------------------------------------------
    // Session 4 (fresh session): New card terms are searchable + access-denied handling.
    // -------------------------------------------------------------------------
    {
        let mut session4 = AgentSession::new(&db, &provider, PROVIDER_SECRET);

        // Newly refreshed terms (`circuit breaker dlq replay`) immediately find the anchor doc.
        let refreshed_hit = session4.rediscover_and_answer("circuit breaker dlq replay");
        assert_eq!(
            refreshed_hit,
            RediscoveryOutcome::AnsweredFromLiveSource {
                id: anchor_id.clone(),
                canonical_key: "gdrive:doc:WEBHOOK_ANCHOR".into(),
                live_detail:
                    "Current live policy (rev 2): open circuit breaker after 50% 5xx over 60s and replay from DLQ."
                        .into(),
                revision: "2".into(),
                refreshed_card: false,
            }
        );

        // Newly indexed Jira ticket is now rediscoverable from vague description.
        let jira_hit = session4.rediscover_and_answer("partner 502 thundering herd ticket");
        assert_eq!(
            jira_hit,
            RediscoveryOutcome::AnsweredFromLiveSource {
                id: jira_id.clone(),
                canonical_key: "jira:issue:acme.atlassian.net:ENG-123".into(),
                live_detail: "Live status: In Progress, assigned to Platform SRE.".into(),
                revision: "1".into(),
                refreshed_card: false,
            }
        );
    }

    // Revoke access on the live provider -> Session 5 finds the pointer but does NOT treat the
    // local card as live source truth.
    provider.set_accessible("google_drive", "WEBHOOK_ANCHOR", false);
    {
        let mut session5 = AgentSession::new(&db, &provider, PROVIDER_SECRET);
        let denied = session5.rediscover_and_answer("circuit breaker dlq replay");
        assert_eq!(
            denied,
            RediscoveryOutcome::PointerFoundButSourceUnavailable {
                id: anchor_id.clone(),
                canonical_key: "gdrive:doc:WEBHOOK_ANCHOR".into(),
                primary_uri: doc_edit_uri.into(),
                card_was_not_treated_as_live_truth: true,
            }
        );

        let doctor = session5.run_json(&["doctor"], None);
        assert_eq!(doctor["healthy"], true);
        assert_eq!(doctor["foreign_key_integrity"], true);
        assert_eq!(doctor["fts_integrity"], true);
    }

    // Boundary audit: zero source bodies and zero provider credentials persisted in SQLite.
    assert_sqlite_contains_no_forbidden_strings(
        &db,
        &[
            PROVIDER_SECRET,
            ANCHOR_BODY_REV1_SENTINEL,
            ANCHOR_BODY_REV2_SENTINEL,
            JIRA_BODY_SENTINEL,
        ],
    );

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bundled_skill_aligns_with_tested_cli_workflow_and_boundaries() {
    let skill_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/artmark/SKILL.md");
    let skill = fs::read_to_string(skill_path).unwrap();

    for expected in [
        "artmark --version",
        "artmark add <uri> --explicit --json-input - --json",
        "artmark add <uri> --explicit --json",
        "artmark search <query> --json",
        "artmark get <id> --json",
        "artmark get <id> --no-track --json",
        "artmark index <id> --json-input - --json",
        "artmark recent --state registered --json",
        "Do not automatically register every URL or file path the user pastes",
        "Do not answer substantive questions about current source contents solely from `summary` or `search_text`",
        "artmark found the artifact's pointer but the current provider could not be accessed",
    ] {
        assert!(
            skill.contains(expected),
            "skills/artmark/SKILL.md is missing expected workflow contract: {expected}"
        );
    }
}
