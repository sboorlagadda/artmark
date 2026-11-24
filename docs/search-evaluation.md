# Offline FTS5 Search Evaluation & Decision Record

Evaluation for Issue `#10` ("Evaluate and improve offline FTS5 search with real queries"), serving as the empirical gate before considering optional semantic search (`#16` / `#17`).

## Corpus

The reproducible fixture lives at [`tests/fixtures/search_eval.json`](../tests/fixtures/search_eval.json) and is exercised automatically by `search_evaluation_corpus_achieves_target_retrieval_quality` in [`tests/cli.rs`](../tests/cli.rs).

- **22 anonymized artifacts** spanning Google Docs, Google Sheets, Google Slides, Figma files, GitHub issues/PRs/repositories, Notion pages, Slack threads, web pages, and local filesystem files (including one pointer-only `registered` artifact and older non-explicit entries).
- **29 queries** across 6 categories:
  - `exact` (4): URL alias (`/view` vs `/edit`), canonical key, exact title (pointer-only), external ID.
  - `vague_conversational` (6): natural phrasing with filler words (e.g., *"my framing doc for checkout"*, *"where is the doc about migrating to SAML"*, *"what was that deck for architecture review"*).
  - `stemming` (8): inflected/morphological queries (e.g., *"webhook retry design"*, *"migrating identities"*, *"rotating certificates"*, *"configuring connection pools"*).
  - `precision` (6): multi-term queries with close distractors (e.g., *"framing doc for payments checkout"* against `"API Doc Style Guidelines"` and older checkout scratch notes; *"where is the retry backoff"*).
  - `filtered` (4): `--provider`, `--kind`, and `--topic` filters.
  - `lexical_boundary` (1): intentional pure-synonym query (*"billing meltdown"* targeting a `"Payment Gateway Timeout Postmortem"` card that omits both words) to measure the honest boundary of lexical search.

## Baseline (`v0.3.0`) vs. Upgraded (`v0.4.0`)

`v0.3.0` used default FTS5 `unicode61` tokenization, `OR`-joined `"word"*` prefix queries without stopword removal, and rank-reciprocal scores (`0.7 / (rank + 1)`).

`v0.4.0` (schema v2) upgrades `artifact_fts` to `tokenize = 'porter unicode61'`, strips conversational English stopwords (with fallback if all tokens are stopwords), quotes each token as `("term" OR "term"*)` for tokens of 3+ characters (avoiding 1–2 char prefix noise and FTS5 operator syntax collisions), runs an **AND-first** pass (`score ∈ (0.55, 0.90)`) with **OR fallback** (`score ∈ (0.05, 0.50)`), preserves real weighted BM25 ranking (`title=8.0, summary=3.0, topics=2.5, entities=2.5, search_text=1.5, tags=1.5`), and short-circuits exact locator/key lookups to avoid noisy URL-token matches.

| Metric (28 lexical queries) | `v0.3.0` baseline | `v0.4.0` upgraded |
| --- | ---: | ---: |
| **Hit@1** | 25 / 28 (89.3%) | **28 / 28 (100.0%)** |
| **Hit@3** | 26 / 28 (92.9%) | **28 / 28 (100.0%)** |
| **MRR** | 0.911 | **1.000** |

### Concrete baseline failures fixed

1. **Zero-hit morphological misses:** *"migrating identities"* and *"rotating certificates"* returned `[]` in `v0.3.0` because `unicode61` prefix matching cannot link `migrating`↔`migration`, `identities`↔`identity`, or `rotating`↔`rotation`. Both now rank `#1` as clean single-hit AND matches (`0.8265` and `0.8258`).
2. **Stopword + inflection misranking:** *"where is the retry backoff"* ranked the GitHub bug report ahead of the Webhook Retries RFC (`RANK=2`) and returned an unrelated Figma file at `#3` via `"the"`. Stripping stopwords + Porter stemming puts the RFC at `#1` (`0.8147`) and eliminates the unrelated result.
3. **Conversational and URL noise:** Stopwords (`the`, `for`, `about`, `to`) and URL path segments (`docs`, `d`, `pr`) previously filled top-3 slots with unrelated entries. Exact locator hits now return cleanly without URL-token noise, and conversational queries return only lexically relevant cards.
4. **Pre-filter score distortion:** Filtered queries like `search onboarding --provider figma` previously reported `score = 0.35` because `0.7 / (rank + 1)` divided by the unfiltered FTS rank. Real BM25 scores (`0.7709`) now reflect actual match strength regardless of filter selectivity.

### Explicit-save and recency boost evaluation

- **Recency boost — rejected:** Anchor documents (e.g., an architecture doc saved months ago) are frequently older than newer distractors (e.g., a recent style guide or scratch note). Boosting recency risks hiding durable anchor docs behind newer incidental mentions.
- **Explicit-save boost — deferred:** In `v0.3.0+`, artmark no longer silently auto-registers pasted links, so newly saved artifacts are explicitly saved by default. On mixed databases with older implicit notes, weighted BM25 (especially `title=8.0` and `topics/entities=2.5`) already separates canonical anchor docs from scratch notes (`checkout_framing` beat `implicit_old_notes` on every checkout query without heuristics). Adding a heuristic boost is unnecessary unless future user reports show ties hurting recall.

## Decision & gate for semantic search (`#16`)

**Decision: stay 100% local lexical FTS5 (`porter unicode61` + stopword stripping + AND-first/OR-fallback + weighted BM25) with no embeddings.**

The single remaining miss in the fixture (`"billing meltdown"` → `"Payment Gateway Timeout Postmortem"`) is a pure conceptual synonym gap where the card contained neither `"billing"` nor `"meltdown"`. Notice that under `v0.4.0`, the weak partial match (`api_doc_guidelines`, matching `"billing"` only) drops clearly to the OR-fallback tier (`0.3316` instead of `0.70`), giving agents a much clearer signal that no strong match was found.

Per the roadmap, optional local embeddings (`#16` / `#17`) remain gated behind real post-launch evidence showing that users repeatedly hit conceptual synonym misses that agent-written `search_text` and lexical FTS5 cannot solve.
