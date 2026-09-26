# Artmark

## Local Agent-Native Artifact Registry

**Status:** v0.1 design specification  
**Primary interface:** MCP + CLI  
**Deployment:** local-only, single-user, zero-daemon  
**Storage:** SQLite  
**Search:** SQLite FTS5 + optional sqlite-vec  
**Core abstraction:** artifact catalog entry, not artifact copy

---

# 1. Product definition

Artmark is a local artifact registry for AI agents.

It remembers **what artifacts exist, what they are about, and how an agent can retrieve them again**.

It does **not** store or synchronize the underlying artifacts.

The authoritative artifact remains in its original system:

```text
Google Drive
Figma
GitHub
Notion
Slack
local filesystem
web
etc.
```

Artmark stores a durable catalog entry pointing to that artifact.

The mental model is:

```text
Artmark = Yellow Pages for artifacts
```

An agent uses Artmark to answer:

```text
"Have I seen something relevant to this before?"
"What was that document called?"
"Which Figma file contained the onboarding work?"
"Where is the architecture doc about SAML?"
"How do I retrieve it again?"
```

Once the artifact is identified, the agent uses its existing provider-specific MCP/plugin/tool to retrieve the live source.

---

# 2. Core workflow

Artmark sits between agent sessions and external artifact systems.

```text
                    USER / SESSION

                         │
                         │ URL / file / artifact
                         ▼

                       AGENT
                         │
             identifies artifact/provider
                         │
                         ▼
              existing provider tool
         Drive MCP / Figma MCP / GitHub MCP
                         │
                  inspect artifact
                         │
                         ▼
                generate catalog card
                         │
                         ▼
                    ┌─────────┐
                    │ Artmark │
                    └────┬────┘
                         │
          metadata + search representation
                         │
                     SQLite
                  FTS5 + vectors
```

Later:

```text
User:
"Find the document where we discussed enterprise SAML migration."

              │
              ▼
        artifact_search
              │
              ▼
          Artmark
              │
       likely artifact
              │
              ▼
        Agent identifies
       google_drive:doc:ABC
              │
              ▼
       Google Drive MCP
              │
              ▼
         current source
```

Artmark's job ends when it successfully directs the agent to the correct artifact.

---

# 3. Key architectural principle

Artmark stores:

```text
artifact identity
artifact locator
metadata
search-oriented description
semantic representation
provenance
freshness hints
```

Artmark does not normally store:

```text
full Google Doc contents
Figma document contents
complete GitHub repository contents
binary PDFs
images
attachments
document snapshots
```

This distinction is fundamental.

---

# 4. Why the agent performs ingestion

Artmark must not become a collection of provider integrations.

The agent already has tools capable of accessing providers.

Example:

```text
User pastes:
https://docs.google.com/document/d/ABC/edit

Agent recognizes:
Google Doc

Agent calls:
Google Drive MCP

Agent learns:
title
document type
possibly owner
revision
last modified
contents/topics/context

Agent generates:
Artmark catalog entry

Agent calls:
artifact_register
```

Artmark never needs the user's Google OAuth credentials.

The same pattern works for:

```text
Figma
GitHub
Notion
Linear
Slack
Dropbox
web pages
local files
future providers
```

Provider support therefore largely lives in the **agent's tool ecosystem**, not Artmark.

---

# 5. Primary use cases

## 5.1 Explicit bookmarking

User:

```text
Save this:
https://...
```

Agent:

```text
resolve artifact
understand enough for later retrieval
register with Artmark
```

No separate bookmark-manager UI.

---

## 5.2 Incidental artifacts during sessions

User provides:

```text
Google Docs
Figma links
GitHub issues
PRs
web pages
PDF references
Notion pages
etc.
```

The agent can register appropriate artifacts as they appear.

This provides persistence across sessions.

---

## 5.3 Future discovery

User:

```text
Find the design file where we explored
single-screen onboarding.
```

Agent:

```text
artifact_search("single screen onboarding")
```

Artmark returns likely artifacts.

Agent obtains the live artifact using the appropriate provider tool.

---

## 5.4 Avoid repeated prompting

Instead of:

```text
Here are the five docs you need:
https://...
https://...
https://...
```

the user can say:

```text
Use the architecture docs related to Phoenix.
```

The agent discovers them through Artmark.

---

# 6. Non-goals

Artmark v1 is not:

```text
a document database
a RAG knowledge base
a Google Drive sync engine
a Figma backup tool
a web crawler
a bookmark-reading application
an LLM memory system
a chat-history database
a cloud service
a multi-user permissions system
a vector-database server
```

It must remain narrow.

---

# 7. Terminology

## Artifact

An external durable object useful to the user or an agent.

Examples:

```text
Google Doc
Figma design
GitHub repository
GitHub issue
GitHub pull request
Notion page
Slack thread
web page
PDF
local project directory
spreadsheet
presentation
```

---

## Artifact record

The local Artmark record describing an artifact.

---

## Locator

Information required to find the source again.

Examples:

```text
URL
provider object ID
repository + issue number
filesystem path
Figma file key
Google Drive file ID
```

---

## Search card

The compact semantic representation Artmark stores for retrieval.

The central field is:

```text
search_text
```

The search card is **not a source snapshot**.

It is an agent-generated description optimized for rediscovery.

---

# 8. Search card philosophy

The most important design decision in Artmark is the quality of `search_text`.

An artifact may contain 50,000 tokens.

Artmark might store only 300–1,500 tokens describing it.

Example source:

```text
40-page architecture document
```

Artmark search representation:

```text
Title: Enterprise Authentication Architecture V2

Purpose:
Technical architecture for enterprise authentication
and organization provisioning.

Topics:
SAML
SCIM
Okta
Azure AD
IdP discovery
account migration
identity federation
enterprise onboarding
organization provisioning

Important content:
- migration from legacy SAML setup
- automatic identity provider discovery
- SCIM-based lifecycle management
- account merge behavior during migration
- rollout risks for existing enterprise tenants

Relevant projects:
Project Phoenix
Enterprise Platform

Likely useful for:
authentication architecture
SAML migration
enterprise provisioning
customer migration
identity systems
```

The agent embeds and indexes this representation.

It does not need to retain the original architecture document.

---

# 9. Desired property of `search_text`

The test is not:

```text
"Does this summarize the artifact accurately?"
```

The better test is:

```text
"If I vaguely remember this artifact six months later,
what phrases might I use to search for it?"
```

Therefore `search_text` should emphasize:

```text
subject matter
project names
people/team names when appropriate
technologies
features
decisions
problems being solved
important terminology
artifact purpose
related systems
alternate terminology
likely future search phrases
```

It should not merely compress the document.

It should improve discoverability.

---

# 10. Source metadata vs agent metadata

These must remain separate.

## Source metadata

Facts obtained from the underlying provider.

Examples:

```text
title
provider object ID
type
canonical URL
revision/version
source modified timestamp
repository
issue number
file key
mime type
```

## Agent-generated metadata

Interpretive catalog information.

Examples:

```text
summary
search_text
topics
entities
keywords
project names
suggested tags
```

This distinction enables later regeneration of semantic metadata without changing source identity.

---

# 11. Local storage

Artmark should require exactly one primary durable file:

```text
~/.artmark/artmark.db
```

Optional:

```text
~/.artmark/config.toml
```

Full layout:

```text
~/.artmark/
├── artmark.db
└── config.toml
```

No artifact directories.

No content cache.

No object store.

No background daemon state.

---

# 12. Storage philosophy

SQLite is the canonical Artmark database.

Within SQLite:

```text
ordinary relational rows = durable state

FTS5 index = rebuildable derived state

vector embeddings = rebuildable derived state
```

This means:

```text
metadata and search_text must survive

FTS tables can be recreated

vector table can be recreated

embeddings can be regenerated
```

---

# 13. Artifact identity

Every Artmark artifact gets an immutable internal ID.

Example:

```text
art_01K9DM7P8MK7...
```

Recommended implementation:

```text
UUIDv7
```

or:

```text
ULID
```

Artmark also stores a canonical external identity.

Examples:

```text
gdrive:doc:1ABC
gdrive:sheet:1XYZ

figma:file:R7T8Q

github:repo:openai/codex
github:issue:openai/codex:328
github:pr:openai/codex:401

notion:page:ABC

web:https://example.com/foo

file:/Users/alice/project/design.pdf
```

Internal identity and external identity are separate.

---

# 14. Canonical identity

The canonical key should represent the external object, not the URL used to reach it.

Example:

```text
https://docs.google.com/document/d/ABC/edit
https://docs.google.com/document/d/ABC/view
```

both become:

```text
gdrive:doc:ABC
```

Likewise:

```text
https://github.com/foo/bar/issues/42
```

becomes:

```text
github:issue:foo/bar:42
```

Canonicalization should be local and deterministic wherever possible.

---

# 15. Aliases

Every URL encountered should optionally be retained as an alias.

Example:

```text
artifact:
gdrive:doc:ABC

aliases:
https://docs.google.com/document/d/ABC
https://docs.google.com/document/d/ABC/edit
```

This means pasting either URL later identifies the existing artifact immediately.

---

# 16. Artifact states

Keep states minimal.

## registered

The artifact is known, but has little semantic metadata.

Example:

```text
URL
provider
type
canonical ID
```

## indexed

The agent has inspected the artifact and generated a usable search card.

## unreachable

Previously known artifact cannot currently be retrieved.

This should not automatically remove it.

---

# 17. Artifact schema

Conceptual object:

```json
{
  "id": "art_01K9DM7...",

  "canonical_key": "gdrive:doc:ABC",

  "kind": "google_doc",
  "provider": "google_drive",

  "state": "indexed",

  "primary_uri": "https://docs.google.com/document/d/ABC/edit",

  "source": {
    "external_id": "ABC",
    "revision": "183",
    "updated_at": "2026-09-25T18:15:00Z"
  },

  "source_metadata": {
    "title": "Enterprise Authentication Architecture V2",
    "mime_type": "application/vnd.google-apps.document"
  },

  "catalog": {
    "summary": "Architecture for enterprise identity and provisioning.",

    "topics": [
      "SAML",
      "SCIM",
      "Okta",
      "enterprise authentication"
    ],

    "entities": [
      "Project Phoenix",
      "Azure AD"
    ],

    "search_text": "..."
  },

  "retrieval": {
    "preferred_tool": "google_drive",
    "provider": "google_drive",
    "external_id": "ABC"
  },

  "provenance": {
    "registered_by": "agent",
    "registration_context": "conversation",
    "resolver": "google_drive_mcp"
  },

  "created_at": "...",
  "updated_at": "...",
  "last_seen_at": "...",
  "last_indexed_at": "..."
}
```

---

# 18. Recommended SQL schema

```sql
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,

    canonical_key TEXT NOT NULL UNIQUE,

    kind TEXT NOT NULL,
    provider TEXT,
    state TEXT NOT NULL,

    primary_uri TEXT,

    title TEXT,
    summary TEXT,
    search_text TEXT,

    source_external_id TEXT,
    source_revision TEXT,
    source_updated_at TEXT,

    retrieval_tool TEXT,
    retrieval_json TEXT,

    source_metadata_json TEXT,
    catalog_metadata_json TEXT,
    provenance_json TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    last_indexed_at TEXT
);
```

Aliases:

```sql
CREATE TABLE aliases (
    uri TEXT PRIMARY KEY,

    artifact_id TEXT NOT NULL,

    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,

    FOREIGN KEY (artifact_id)
      REFERENCES artifacts(id)
      ON DELETE CASCADE
);
```

Topics:

```sql
CREATE TABLE topics (
    artifact_id TEXT NOT NULL,
    topic TEXT NOT NULL,

    PRIMARY KEY (artifact_id, topic),

    FOREIGN KEY (artifact_id)
      REFERENCES artifacts(id)
      ON DELETE CASCADE
);
```

Entities:

```sql
CREATE TABLE entities (
    artifact_id TEXT NOT NULL,
    entity TEXT NOT NULL,

    PRIMARY KEY (artifact_id, entity),

    FOREIGN KEY (artifact_id)
      REFERENCES artifacts(id)
      ON DELETE CASCADE
);
```

Optional tags:

```sql
CREATE TABLE tags (
    artifact_id TEXT NOT NULL,
    tag TEXT NOT NULL,

    PRIMARY KEY (artifact_id, tag),

    FOREIGN KEY (artifact_id)
      REFERENCES artifacts(id)
      ON DELETE CASCADE
);
```

Index metadata:

```sql
CREATE TABLE index_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

---

# 19. FTS5 index

Create:

```sql
CREATE VIRTUAL TABLE artifact_fts USING fts5(
    artifact_id UNINDEXED,

    title,
    summary,
    search_text,

    topics,
    entities,
    tags
);
```

The FTS document should contain only retrieval-oriented information.

Do not insert full external artifact contents.

---

# 20. Embedding index

Semantic search is optional.

Recommended engine:

```text
sqlite-vec
```

Example:

```sql
CREATE VIRTUAL TABLE artifact_vec USING vec0(
    artifact_id TEXT PRIMARY KEY,
    embedding float[384]
);
```

Exact implementation can vary depending on sqlite-vec binding constraints.

The key invariant is:

```text
one primary vector per artifact
```

for v1.

---

# 21. What gets embedded

Embed the artifact's retrieval card.

Recommended input:

```text
TITLE
Enterprise Authentication Architecture V2

TYPE
Google Doc

SUMMARY
Architecture for enterprise authentication,
identity federation and lifecycle provisioning.

TOPICS
SAML
SCIM
Okta
Azure AD
IdP discovery
account migration

ENTITIES
Project Phoenix

SEARCH DESCRIPTION
<search_text>
```

Do not embed:

```text
full artifact content
raw JSON
URLs alone
timestamps
irrelevant provider metadata
```

---

# 22. Search-card generation contract

The agent should generate approximately:

```text
300–1500 tokens
```

depending on artifact complexity.

Small artifact:

```text
100–400 tokens
```

Large important document:

```text
500–1500 tokens
```

Avoid overly long entries.

---

# 23. Suggested generation prompt

Agent skill should tell the model:

```text
Create a retrieval-oriented catalog entry for this artifact.

The purpose is not to summarize the artifact completely.

The purpose is to make the artifact discoverable later when
the user only vaguely remembers what it contained.

Include:

- the artifact's purpose
- important topics
- projects and product areas
- meaningful technologies
- important decisions or questions
- notable names/entities when relevant
- alternate terminology someone may search for
- problems this artifact helps solve
- likely future search phrases

Do not reproduce large portions of the source.

Do not include secrets, credentials, authentication tokens,
or unnecessary sensitive source text.
```

---

# 24. Important search_text rule

The agent may introduce useful semantic terminology not literally present in the source.

For example, source content may repeatedly say:

```text
identity-provider setup
```

Agent may include:

```text
SAML configuration
enterprise SSO
identity federation
IdP onboarding
```

This improves future rediscovery.

However, agent-generated interpretation must remain clearly distinguished from source metadata.

---

# 25. Retrieval pipeline

Given:

```text
"where was that doc about migrating old enterprise SSO customers?"
```

Run:

```text
1. exact canonical/URI lookup
2. FTS search
3. semantic vector search
4. rank fusion
5. metadata boosts
6. return compact artifact candidates
```

---

# 26. Exact lookup

Before fuzzy search:

```text
canonical key
exact URI alias
exact title
provider external ID
```

should be checked.

Exact identity matches rank above semantic results.

---

# 27. Hybrid ranking

Recommended candidate counts:

```text
FTS candidates: 30
vector candidates: 30
```

Merge with Reciprocal Rank Fusion:

```text
score = Σ 1 / (k + rank)
```

Recommended:

```text
k = 60
```

Then optionally add small deterministic boosts.

Examples:

```text
exact provider match
exact topic match
recently seen
explicitly saved
```

Avoid complicated ranking logic initially.

---

# 28. Search response

Search should return **artifact cards**, not contents.

Example:

```json
{
  "id": "art_01K9...",

  "title": "Enterprise Authentication Architecture V2",

  "kind": "google_doc",
  "provider": "google_drive",

  "summary": "Architecture for enterprise identity and provisioning.",

  "primary_uri": "https://docs.google.com/...",

  "topics": [
    "SAML",
    "SCIM",
    "identity migration"
  ],

  "source_updated_at": "2026-09-25T18:15:00Z",

  "last_indexed_at": "2026-09-26T09:00:00Z",

  "score": 0.82
}
```

Do not return complete `search_text` unless requested.

This keeps model context small.

---

# 29. Retrieval hint

Each artifact should contain enough information for the agent to know how to retrieve the live object.

Example:

```json
{
  "preferred_tool": "google_drive",
  "provider": "google_drive",
  "external_id": "ABC",
  "uri": "https://docs.google.com/document/d/ABC/edit"
}
```

Another:

```json
{
  "preferred_tool": "github",
  "provider": "github",
  "owner": "openai",
  "repo": "codex",
  "object_type": "issue",
  "number": 123
}
```

Another:

```json
{
  "preferred_tool": "figma",
  "provider": "figma",
  "file_key": "ABCD1234"
}
```

`preferred_tool` is a hint, not a hard requirement.

Agent environments may expose different tool names.

---

# 30. Freshness model

Artmark does not promise the catalog entry is current.

It records:

```text
source_updated_at
source_revision
last_indexed_at
last_seen_at
```

The live provider remains authoritative.

Therefore:

```text
stale metadata = potentially worse retrieval

stale metadata ≠ incorrect current artifact contents
```

This is an intentional simplification.

---

# 31. Opportunistic refresh

No background synchronization is required.

Preferred model:

```text
search Artmark
     ↓
find artifact
     ↓
retrieve from provider
     ↓
agent notices meaningful source changes
     ↓
refresh Artmark entry
```

This keeps the catalog healthier naturally during real use.

---

# 32. Registration modes

Artmark supports two levels.

## Quick registration

Agent knows only:

```text
URI
provider
type
canonical identity
title if obvious
```

Call:

```text
artifact_register
```

State:

```text
registered
```

Useful when an artifact is merely mentioned.

---

## Indexed registration

Agent has inspected the object.

Includes:

```text
source metadata
summary
topics
entities
search_text
retrieval hints
```

State:

```text
indexed
```

This is the preferred state for artifacts that matter.

---

# 33. Incidental-link policy

Do not deeply inspect every link automatically.

Recommended agent behavior:

### Explicit save

User says:

```text
save this
remember this
bookmark this
add this to Artmark
```

Then:

```text
deeply index immediately
```

### Artifact actively used

Agent reads it as part of current work.

Then:

```text
index it because content is already available
```

### Artifact merely mentioned

Then:

```text
quick-register only
```

This avoids:

```text
unnecessary MCP calls
slow sessions
garbage metadata
costly indexing
```

---

# 34. Deduplication

Order:

```text
1. canonical_key
2. exact alias
3. provider + source external ID
4. normalized generic URL
5. otherwise create new artifact
```

Do not deduplicate solely based on search text similarity.

---

# 35. Re-registration behavior

If artifact already exists:

```text
update last_seen_at
add newly observed alias
merge non-conflicting source metadata
```

If caller provides a fresh catalog card:

```text
replace agent-generated semantic fields
update last_indexed_at
regenerate embedding
update FTS
```

---

# 36. Catalog history

v1 does not need full revision history.

Keep only current Artmark metadata.

Future option:

```text
artifact_versions
```

may preserve previous search cards if useful.

Do not build this initially.

---

# 37. CLI name

Primary:

```bash
artmark
```

Optional short alias:

```bash
am
```

Examples:

```bash
artmark add <uri>

artmark search "enterprise authentication"

artmark get <id>

artmark forget <id>

artmark reindex

artmark doctor

artmark mcp
```

---

# 38. `artmark add`

Quick registration.

Example:

```bash
artmark add \
  https://docs.google.com/document/d/ABC/edit
```

Artmark:

```text
canonicalizes URL
detects provider if possible
deduplicates
creates artifact
```

No provider API calls.

Potential output:

```text
art_01K9...
Google Drive document
registered
```

JSON:

```bash
artmark add URL --json
```

returns:

```json
{
  "id": "art_01K9...",
  "canonical_key": "gdrive:doc:ABC",
  "state": "registered"
}
```

---

# 39. `artmark index`

Optional CLI path for external agents/scripts.

Example:

```bash
artmark index art_01K9... --json-input entry.json
```

Normally MCP agents call the equivalent tool.

---

# 40. `artmark search`

Examples:

```bash
artmark search "checkout redesign"
```

```bash
artmark search "SAML migration" \
  --provider google_drive
```

```bash
artmark search "onboarding" \
  --kind figma_file
```

Options:

```text
--provider
--kind
--topic
--tag
--limit
--json
--lexical-only
--semantic-only
```

---

# 41. `artmark get`

Example:

```bash
artmark get art_01K9...
```

Shows catalog metadata and retrieval information.

It does not fetch the upstream artifact.

---

# 42. `artmark open`

Optional convenience command:

```bash
artmark open art_01K9...
```

Behavior:

```text
print primary URI
```

or optionally delegate to OS browser.

Not required for core v1.

---

# 43. `artmark forget`

Deletes catalog entry only.

```bash
artmark forget art_01K9...
```

It never modifies the underlying source artifact.

This semantic distinction is important.

Use:

```text
forget
```

rather than:

```text
delete artifact
```

---

# 44. MCP interface

Keep extremely small.

Recommended four tools:

```text
artifact_register
artifact_index
artifact_search
artifact_get
```

Optionally:

```text
artifact_forget
```

---

# 45. `artifact_register`

Purpose:

```text
cheap registration of a pointer
```

Input:

```json
{
  "uri": "https://docs.google.com/document/d/ABC/edit",

  "kind": "google_doc",
  "provider": "google_drive",

  "title": "optional title",

  "canonical_key": "optional if agent knows it",

  "retrieval": {
    "external_id": "ABC"
  }
}
```

Artmark canonicalizes further where possible.

Returns artifact ID.

---

# 46. `artifact_index`

Purpose:

```text
create/update searchable semantic catalog entry
```

Input:

```json
{
  "id": "art_01K9...",

  "source_metadata": {
    "title": "Enterprise Authentication Architecture V2",
    "revision": "183",
    "updated_at": "..."
  },

  "catalog": {
    "summary": "...",

    "topics": [
      "SAML",
      "SCIM"
    ],

    "entities": [
      "Project Phoenix"
    ],

    "search_text": "..."
  },

  "retrieval": {
    "provider": "google_drive",
    "external_id": "ABC"
  },

  "resolver": "google_drive_mcp"
}
```

It may also accept URI/canonical key instead of Artmark ID.

---

# 47. Combined upsert

Implementation may internally collapse `register` and `index` into a single upsert.

But exposing both concepts to agents has a useful behavioral effect:

```text
register = cheap
index = meaningful inspection performed
```

That distinction helps prevent excessive provider calls.

---

# 48. `artifact_search`

Input:

```json
{
  "query": "enterprise SAML migration",

  "limit": 10,

  "providers": [
    "google_drive",
    "figma"
  ],

  "kinds": [
    "google_doc"
  ]
}
```

Output:

```json
{
  "results": [
    {
      "id": "art_...",
      "title": "...",
      "kind": "...",
      "provider": "...",
      "summary": "...",
      "primary_uri": "...",
      "score": 0.87
    }
  ]
}
```

---

# 49. `artifact_get`

Input:

```json
{
  "id": "art_..."
}
```

Output:

```text
source metadata
catalog metadata
retrieval hints
aliases
freshness timestamps
```

No external retrieval occurs.

---

# 50. Agent skill

Artmark should ship with a canonical agent skill.

Suggested behavior:

```text
# Artmark

Artmark is a persistent local registry of artifacts.

Use it to remember what external artifacts exist and how to find them.

Artmark does not contain authoritative artifact contents.

When the user explicitly asks to save or remember an artifact:

1. Identify the artifact's provider and canonical identity.
2. Use the best available provider tool to inspect it.
3. Generate retrieval-oriented metadata.
4. Register/index it with Artmark.

When an artifact is already being read for the current task:

1. Register or refresh it in Artmark at low additional cost.

When an artifact is merely mentioned:

1. Register its identity cheaply if it appears durable and potentially useful.
2. Do not perform expensive retrieval solely to index it unless necessary.

Before asking the user for a previously provided document, design, repository, issue, or other artifact:

1. Search Artmark.
2. Inspect likely results.
3. Retrieve the selected artifact from the authoritative provider using existing tools.

Never treat Artmark's summary or search_text as authoritative current artifact contents.

If current details matter, read the upstream artifact.
```

---

# 51. Search-card agent instructions

Detailed indexing instructions:

```text
When creating search_text:

Write for future retrieval rather than for immediate comprehension.

Imagine the user returns six months later and remembers only:

- what problem the artifact addressed
- one feature mentioned inside
- a project name
- a technology
- a decision
- a vague phrase describing the work

Capture enough semantic anchors that those queries can find this artifact.

Prefer meaningful concepts over exhaustive details.

Do not copy entire source sections.

Do not store secrets.

Do not invent factual claims that are unsupported by the artifact.

Synonyms or generalized terminology may be included when they
improve searchability, but they should not distort the artifact's meaning.
```

---

# 52. Example Google Doc entry

```json
{
  "canonical_key": "gdrive:doc:1ABC",

  "kind": "google_doc",
  "provider": "google_drive",

  "primary_uri": "https://docs.google.com/document/d/1ABC/edit",

  "title": "Enterprise Authentication Architecture V2",

  "summary": "Architecture document covering enterprise authentication and provisioning.",

  "topics": [
    "SAML",
    "SCIM",
    "Okta",
    "Azure AD",
    "enterprise SSO",
    "identity migration"
  ],

  "entities": [
    "Project Phoenix"
  ],

  "search_text": "Technical architecture for enterprise authentication. Covers SAML configuration, IdP discovery, Okta and Azure AD integration, SCIM lifecycle management, migrating existing enterprise customers from the legacy authentication system, organization provisioning, account linking, rollout risks and migration sequencing. Useful for questions about enterprise SSO, identity federation, customer authentication migration, SAML onboarding, and provisioning architecture.",

  "retrieval": {
    "provider": "google_drive",
    "external_id": "1ABC"
  }
}
```

---

# 53. Example Figma entry

```json
{
  "canonical_key": "figma:file:F123",

  "kind": "figma_file",
  "provider": "figma",

  "title": "Onboarding Exploration V3",

  "summary": "Design explorations for simplifying initial workspace onboarding.",

  "topics": [
    "onboarding",
    "activation",
    "workspace creation",
    "invite flow",
    "first-run experience"
  ],

  "search_text": "Figma design exploring a simplified single-page onboarding flow. Includes workspace creation, team invitation, template selection, empty states, activation experiments, progressive disclosure and alternative first-run flows. Related to reducing onboarding drop-off and increasing first-session activation.",

  "retrieval": {
    "provider": "figma",
    "file_key": "F123"
  }
}
```

---

# 54. Example GitHub issue

```json
{
  "canonical_key": "github:issue:acme/app:382",

  "kind": "github_issue",
  "provider": "github",

  "title": "Retry failed webhook deliveries",

  "summary": "Proposal and implementation discussion for webhook retry behavior.",

  "topics": [
    "webhooks",
    "retry",
    "backoff",
    "delivery failures"
  ],

  "search_text": "GitHub issue discussing retry behavior for failed webhook deliveries, exponential backoff, idempotency, delivery status tracking, dead-letter handling, retry limits and operational visibility. Relevant to webhook reliability and event delivery architecture.",

  "retrieval": {
    "provider": "github",
    "owner": "acme",
    "repo": "app",
    "number": 382
  }
}
```

---

# 55. Generic web URLs

Artmark can support normal web bookmarks.

For unknown URL:

```text
web:<normalized-url>
```

The agent may use a browser/web tool to inspect the page.

It then produces the same catalog representation.

Artmark itself should not become a crawler.

---

# 56. Local files

Canonical key:

```text
file:/absolute/path
```

For local file indexing, the agent/harness may inspect it using available filesystem tooling.

Store:

```text
path
filename
file type
size if useful
mtime
search metadata
```

Do not copy the file.

---

# 57. Local paths can move

This is one weakness of filesystem identities.

Future options:

```text
content fingerprint
inode/device metadata
project-relative identity
git repository + relative path
```

Do not over-engineer v1.

---

# 58. Embeddings

Embedding support must remain optional.

Configuration:

```toml
[embedding]
provider = "none"
```

or:

```toml
[embedding]
provider = "command"
dimensions = 384
command = ["my-local-embedder"]
model = "bge-small-en-v1.5"
```

---

# 59. Embedder abstraction

Conceptually:

```rust
trait Embedder {
    fn dimensions(&self) -> usize;
    fn model_id(&self) -> &str;

    async fn embed_query(
        &self,
        query: &str
    ) -> Result<Vec<f32>>;

    async fn embed_document(
        &self,
        document: &str
    ) -> Result<Vec<f32>>;
}
```

Artmark must work perfectly without an embedder.

---

# 60. Why no embedded model runtime initially

Do not make Artmark ship:

```text
PyTorch
ONNX runtime
llama.cpp
large model weights
GPU libraries
```

The core binary should remain tiny.

Embedding generation can be supplied externally.

---

# 61. Configuration example

```toml
version = 1

database = "~/.artmark/artmark.db"

[index]
fts = true
semantic = true

[embedding]
provider = "command"
model = "bge-small-en-v1.5"
dimensions = 384
command = ["local-embed"]

[search]
lexical_candidates = 30
semantic_candidates = 30
rrf_k = 60

[agent]
quick_register_incidental = true
```

---

# 62. Database versioning

Use schema migrations.

Store:

```text
PRAGMA user_version
```

or explicit migrations table.

Never make destructive automatic migrations without backup.

Before structural migration:

```text
artmark.db.bak
```

may be created.

---

# 63. Backup

The whole product should be backupable with:

```text
one SQLite file
```

Export additionally:

```bash
artmark export > artmark.jsonl
```

Import:

```bash
artmark import artmark.jsonl
```

This gives portability independent of SQLite schema evolution.

---

# 64. JSONL export

Each line should represent one complete artifact record.

Exclude vector values by default.

Example:

```json
{"schema_version":1,"id":"art_...","canonical_key":"...","catalog":{...}}
```

Embeddings should be regenerated after import.

---

# 65. Reindex

```bash
artmark reindex
```

should:

```text
rebuild FTS5
regenerate vector index if configured
preserve artifact records
```

It must never require external providers.

Everything necessary for Artmark's search index comes from the catalog records themselves.

This is an important invariant.

---

# 66. Core invariant

This should always be possible:

```text
Delete all FTS/vector derived structures

Run:
artmark reindex

All registered artifacts become searchable again
without contacting Google, Figma, GitHub, etc.
```

---

# 67. Refresh

A separate operation can update semantic metadata after live retrieval:

```text
artifact_index
```

or CLI:

```bash
artmark refresh-metadata <id> --stdin-json
```

But Artmark itself does not retrieve the artifact.

---

# 68. Security

Default directory:

```text
~/.artmark
```

Permissions:

```text
0700
```

Database:

```text
0600
```

Never store:

```text
OAuth credentials
session cookies
access tokens
API keys
authorization headers
signed URLs when avoidable
```

---

# 69. Sensitive metadata

Since `search_text` is durable, the agent should avoid placing unnecessary secrets into it.

Examples to omit:

```text
passwords
API tokens
customer secrets
private keys
authentication cookies
financial account numbers
```

The search card should describe the artifact, not duplicate sensitive payloads.

---

# 70. Source access remains authoritative

Finding an Artmark entry does not imply current access to the source.

Example:

```text
Artmark says:
Google Doc ABC exists.

Google Drive MCP says:
Access denied.
```

The provider wins.

Artmark should never bypass provider permissions.

---

# 71. Deletion semantics

When:

```bash
artmark forget ARTIFACT
```

Artmark deletes:

```text
catalog record
aliases
topics
entities
tags
FTS entry
vector entry
```

It does not touch:

```text
Google Doc
Figma file
GitHub issue
local file
web page
```

---

# 72. CLI output rules

All commands support:

```text
--json
```

With `--json`:

```text
stdout = valid JSON only
stderr = diagnostics
```

This makes CLI use agent-friendly.

---

# 73. Exit codes

Suggested:

```text
0 success
1 generic error
2 invalid arguments
3 artifact not found
4 database error
5 embedding unavailable
6 invalid catalog input
```

---

# 74. Technology recommendation

Preferred:

```text
Rust
SQLite
FTS5
sqlite-vec
stdio MCP
```

Suggested crates:

```text
clap
rusqlite
serde
serde_json
tokio
rmcp
uuid
chrono
url
sha2
```

---

# 75. No daemon requirement

MCP usage:

```bash
artmark mcp
```

starts a stdio MCP process.

CLI usage opens SQLite directly.

No:

```text
background service
HTTP port
Docker container
Redis
Postgres
worker process
```

---

# 76. Concurrency

Use SQLite WAL:

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA synchronous=NORMAL;
PRAGMA busy_timeout=5000;
```

This should comfortably handle:

```text
multiple agent processes
CLI usage
MCP usage
```

for a personal local registry.

---

# 77. Search filters

Support:

```text
provider
kind
topic
tag
state
created_after
seen_after
indexed_after
```

Example:

```bash
artmark search "onboarding" \
    --provider figma
```

---

# 78. Recent artifacts

Convenience:

```bash
artmark recent
```

Useful for agent sessions.

Potential implementation:

```text
ORDER BY last_seen_at DESC
```

---

# 79. Artifact relationships

Do not add graph semantics in v1.

Possible future relation:

```text
related_to
supersedes
derived_from
implements
discusses
```

But retrieval quality should first be validated without them.

---

# 80. Collections

Avoid mandatory folder structures.

Optional lightweight grouping:

```text
tags
project names
topics
```

This is enough initially.

---

# 81. Explicit save flag

Useful field:

```text
pinned BOOLEAN
```

or:

```text
importance
```

But don't overcomplicate ranking.

An explicitly saved artifact may receive a small search ranking boost.

---

# 82. Potential schema field

Add:

```sql
is_explicitly_saved INTEGER NOT NULL DEFAULT 0
```

This lets Artmark distinguish:

```text
user deliberately saved this

vs

agent encountered this incidentally
```

Potentially useful for cleanup later.

---

# 83. Garbage control

An agent-native registry risks collecting junk.

Possible rule:

```text
incidental registered records that never become indexed
and haven't been seen for 180 days
```

can be surfaced by:

```bash
artmark prune --dry-run
```

Do not automatically delete them in v1.

---

# 84. Inspection quality score

Optional later field:

```text
catalog_quality
```

For example:

```text
pointer_only
basic
rich
```

Do not use opaque AI confidence scores.

A deterministic completeness classification is enough.

---

# 85. Doctor command

```bash
artmark doctor
```

Checks:

```text
SQLite integrity
schema version
duplicate canonical keys
dangling aliases
FTS integrity
sqlite-vec availability
embedding dimensions
embedding model identity
missing vectors
records missing search_text
invalid retrieval metadata
```

---

# 86. Statistics

Useful:

```bash
artmark stats
```

Example:

```text
Artifacts: 1,482
Indexed: 1,201
Registered only: 281

Google Drive: 592
Figma: 217
GitHub: 401
Web: 198
Other: 74

Vector index: enabled
Embedding model: bge-small-en-v1.5
```

---

# 87. MVP milestone 1 — registry

Implement:

```text
SQLite schema
IDs
canonicalization
aliases

artmark add
artmark get
artmark forget
artmark recent
```

No search sophistication yet.

---

# 88. MVP milestone 2 — lexical search

Add:

```text
search_text
topics
entities
summary
FTS5

artmark search
```

At this stage the product is already meaningfully useful.

---

# 89. MVP milestone 3 — MCP

Add:

```text
artifact_register
artifact_index
artifact_search
artifact_get

artmark mcp
```

Ship agent skill.

This milestone validates the core agent workflow.

---

# 90. MVP milestone 4 — semantic search

Add:

```text
embedding interface
sqlite-vec
query embeddings
RRF hybrid ranking
```

Do this only after evaluating FTS behavior.

---

# 91. MVP milestone 5 — canonicalizers

Provide local URL parsers for:

```text
Google Docs
Google Sheets
Google Slides
Google Drive
Figma
GitHub repository
GitHub issue
GitHub PR
Notion
Slack
generic URLs
```

These parsers should not call external APIs.

---

# 92. Acceptance test: explicit save

Given:

```text
user asks agent to save a Google Doc
```

Agent:

```text
uses Drive tool
reads document
generates search card
calls artifact_index
```

Later:

```text
search with a concept from the document
```

must return the artifact.

---

# 93. Acceptance test: session persistence

Given:

```text
session A includes Figma link
```

and agent registers it,

in session B:

```text
artifact_search("onboarding design")
```

must be able to find it without the original chat context.

---

# 94. Acceptance test: no cached artifact

Given indexed Google Doc,

after deleting all agent session history,

Artmark should contain:

```text
metadata
search representation
retrieval locator
```

but not the original document body.

---

# 95. Acceptance test: live source correctness

Given:

```text
Artmark catalog created Monday
Google Doc modified Friday
```

Saturday retrieval should:

```text
find via Artmark
fetch from Google Drive
use Friday's source
```

No synchronization step should be required.

---

# 96. Acceptance test: stale catalog

Given outdated `search_text`,

Artmark may still identify the old artifact.

After agent retrieves the updated source and sees important semantic changes:

```text
artifact_index
```

updates the catalog card.

---

# 97. Acceptance test: deduplication

Given:

```text
https://docs.google.com/document/d/ABC/edit

and

https://docs.google.com/document/d/ABC/view
```

Artmark must have exactly one artifact record.

---

# 98. Acceptance test: vector-free mode

Given:

```toml
[embedding]
provider = "none"
```

all operations except semantic retrieval must function normally.

FTS search remains fully available.

---

# 99. Acceptance test: replace embedding model

Given existing artifacts,

changing:

```text
384-dimensional model
```

to:

```text
768-dimensional model
```

must not affect artifact records.

Running:

```bash
artmark reindex --vectors
```

recreates semantic search state.

---

# 100. MVP success criterion

Do not measure success by:

```text
number of stored artifacts
search benchmark scores
vector throughput
```

The important test is behavioral:

```text
Does the user stop repeatedly finding and pasting the same URLs
into agents?
```

A second test:

```text
Can the agent locate artifacts from vague descriptions
that would previously have required searching old chats?
```

If yes, Artmark is solving the intended problem.

---

# 101. Product boundary

The most important sentence in the project documentation should be:

> Artmark remembers how to find your artifacts; it does not replace the systems that contain them.

---

# 102. Architecture invariant

Artmark must never require the underlying artifact to remain searchable.

Once indexed:

```text
Artmark has enough metadata
to rediscover the artifact locally.
```

But whenever the user needs the actual artifact:

```text
the agent retrieves it from the source provider.
```

---

# 103. Second architecture invariant

Artmark must never require provider credentials.

Provider retrieval belongs to:

```text
agent harness
MCP connectors
plugins
CLI tools already available to the agent
```

Artmark receives only the resulting catalog information.

---

# 104. Third architecture invariant

`search_text` is a **catalog representation**, not a cache.

That means:

```text
good:
"Contains discussion of SAML migration, Okta,
SCIM provisioning and legacy tenant rollout."

bad:
copying 15 pages of the authentication document
into search_text.
```

---

# 105. Fourth architecture invariant

Search must remain useful without embeddings.

Architecture:

```text
identity search
     +
FTS5
     +
optional semantic search
```

not:

```text
vector database or nothing
```

---

# 106. Recommended first implementation

Start with:

```text
Rust binary

~/.artmark/artmark.db

SQLite
FTS5

CLI:
  add
  get
  search
  recent
  forget
  doctor
  reindex
  mcp

MCP:
  artifact_register
  artifact_index
  artifact_search
  artifact_get
```

Then dogfood it for a few weeks.

Do not implement embeddings initially unless lexical retrieval clearly feels insufficient.

---

# 107. Likely eventual architecture

```text
                             ┌───────────────────┐
                             │       Agent       │
                             └─────────┬─────────┘
                                       │
                       ┌───────────────┴───────────────┐
                       │                               │
                       ▼                               ▼
                Provider MCPs                     Artmark MCP
            Drive/Figma/GitHub/etc.                    │
                       │                               │
                       │ inspect                       │
                       └───────────┐                   │
                                   ▼                   │
                         catalog representation         │
                                   │                   │
                                   └───────────âindex
                                                       │
                                                ┌──────▼──────┐
                                                │   SQLite    │
                                                │             │
                                                │ metadata    │
                                                │ search_text │
                                                │ FTS5        │
                                            │ sqlite-vec  │
                                                └──────┬──────┘
                                                       │
                                                artifact_search
                                                       │
                                                       ▼
                                                     Agent
                                                       │
                                      retrieve live
                                                       │
                                                       ▼
                                                Provider MCP
```

---

# 108. One-line positioning

**Artmark is a local semantic registry that lets agents remember and rediscover the artifacts they encounter without copying or synchronizing the artifacts themselves.**

---

# 109. Shorter positioning

**Bookmarks for agents.**

Or:

**Your agentrtifact index.**

Or, internally:

**Yellow Pages for artifacts.**

---

# 110. v1 discipline

Whenever deciding whether to add a feature, ask:

```text
Does this help the agent:

1. register an artifact,
2. rediscover an artifact, or
3. know how to retrieve the live artifact?
```

If not, it probably does not belong in Artmark v1.
