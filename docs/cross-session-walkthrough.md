# Cross-session agent workflow walkthrough

This walkthrough demonstrates the shipped CLI + skill workflow across fresh agent sessions using a local SQLite registry and an external provider tool. The exact end-to-end flow—including an external mock provider with revisions, bearer credentials, and access-denied handling—is exercised automatically in [tests/agent_workflow.rs](../tests/agent_workflow.rs):

```bash
cargo test --locked --test agent_workflow
```

## Boundary

- **artmark** stores only canonical pointers, aliases, source metadata facts, retrieval hints, and compact search cards in local SQLite (`~/.artmark/artmark.db`, or `--database` / `ARTMARK_DB`).
- **The agent's existing provider tools** (Google Drive, Figma, GitHub, Jira, browser, filesystem) hold provider authorization and read live source contents.
- artmark never stores source document bodies or provider credentials.

## Reproducible two-session CLI walkthrough

Use an isolated database so you can run the sequence cleanly without touching `~/.artmark/artmark.db`:

```bash
export ARTMARK_DB=/tmp/artmark-demo.db
rm -f "$ARTMARK_DB" "$ARTMARK_DB-wal" "$ARTMARK_DB-shm"
artmark --version
```

### Session A — Explicit save with inspection (atomic pointer + search card)

When the user asks to remember an anchor doc, the agent inspects it with its Drive tool and saves both the pointer and search card in one step:

```bash
artmark add \
  'https://docs.google.com/document/u/0/d/WEBHOOK_ANCHOR/edit' \
  --explicit \
  --json-input - \
  --json <<'EOF'
{
  "source_metadata": {
    "title": "Webhook Retries Framing Doc",
    "revision": "1",
    "updated_at": "2026-10-01T10:00:00Z"
  },
  "catalog": {
    "summary": "Framing doc for outbound webhook retry backoff and delivery guarantees.",
    "search_text": "framing doc outbound webhook retries exponential backoff jitter idempotency keys",
    "topics": ["webhooks", "reliability"],
    "entities": ["Event Bus"],
    "tags": ["framing", "rfc"]
  },
  "retrieval": {
    "provider": "google_drive",
    "external_id": "WEBHOOK_ANCHOR"
  }
}
EOF
```

If an alternate link to the same doc appears later (e.g., `/document/d/WEBHOOK_ANCHOR/view?usp=sharing`), canonicalization deduplicates it onto the same `art_...` record. If a source cannot be inspected yet (for example, an inaccessible ticket), fall back to pointer-only registration and find unindexed entries later with `artmark recent --state registered --json`.

### Session B — Fresh session rediscovery from a vague prompt

In a brand-new session with no chat history, the user asks *"where is my framing doc for webhook retry backoff?"*. The agent searches artmark before searching external providers:

```bash
artmark search 'where is my framing doc for webhook retry backoff' --json
```

It inspects the top match (which records one local lookup in SQLite):

```bash
artmark get gdrive:doc:WEBHOOK_ANCHOR --json
```

Using `retrieval.provider` (`google_drive`) and `retrieval.external_id` (`WEBHOOK_ANCHOR`), the agent fetches the **live** document through its Drive tool before answering.

### Refreshing after source drift & handling access denied

- **Source evolved:** If the live document moved to revision `2` with material new scope (e.g., circuit breakers and DLQ replay), the agent refreshes the card with `artmark index <id> --json-input - --json` so future sessions find the new terminology.
- **Access revoked:** If the provider denies access, the agent reports that artmark found the pointer (`canonical_key` / `primary_uri`) but live source contents were unavailable—never answering about current contents solely from a possibly stale card.
- **Verify health and local usage:**

```bash
artmark stats --json
artmark doctor --json
```
