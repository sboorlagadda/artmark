---
name: artmark
description: Use the local artmark CLI to find and remember durable sources. When a task requires locating a shared file, message, design, diagram, ticket, PR, or similar source from a description, search artmark before available provider tools even if the provider is named. Do not automatically register every URL or path the user pastes. When the user explicitly asks to save a source, inspect it with an available tool and save a search card in one step, falling back to pointer-only only if it cannot be inspected. At the end of a substantive task, ask before saving notable durable sources read during the task.
---

# artmark

artmark is a local bookmark manager for agents.

It keeps durable catalog entries for pointers that would otherwise be lost with the current session: Google Docs, Figma files, GitHub repositories/issues/PRs, PDFs, web pages, local files, and similar durable objects.

artmark stores enough information to **find an artifact again**. It does not store or replace the artifact itself.

The source provider or local file remains authoritative.

Use existing agent tools such as Google Drive, Figma, GitHub, browser, or filesystem tools to inspect and retrieve source contents. Use artmark for durable identity, retrieval metadata, and search.

artmark catalog text is for discovery. Do not treat it as evidence of the current contents of the source artifact.

## Check CLI availability

Before the first artmark operation in a session, run:

```text
artmark --version
```

Do this at most once per session.

If `artmark` is unavailable, do not automatically download, install, upgrade, or replace executables as a side effect of using this skill.

Continue the user's main task when possible. If the user explicitly asked to save something, state clearly that it was not persisted because artmark is unavailable.

If the user explicitly asks to install or upgrade artmark, handle that as a separate setup task using the project's installation instructions.

Use `--json` for output that will be parsed. Pass user-provided URIs as arguments without shell interpolation.

## When to save an artifact

Do not automatically register every URL or file path the user pastes. Many pasted links—such as transient Slack threads, one-off PRs, Jira tickets, or passing docs—are working context rather than durable bookmarks.

Save an artifact only when:

1. **The user explicitly asks** to save, remember, bookmark, or artmark it.
2. **The user approves it at the end of a task.**

### Explicit save (inspect and index in one step)

When the user explicitly asks to save a durable artifact, inspect the source with the best available tool (reusing contents already read in the current task when possible) and save both the pointer and search card atomically:

```text
artmark add <uri> --explicit --json-input - --json
```

If no available tool can read the source, fall back to saving the pointer only (include `--title` if known):

```text
artmark add <uri> --explicit --json
```

Do not invent catalog metadata. Tell the user clearly that only the pointer was saved and that it will not show up in descriptive topic searches until it is indexed.

### End-of-task confirmation for read artifacts

Do not interrupt a task to register links as you encounter them—whether pasted by the user, found during research, or linked from a user-supplied source.

At the end of a substantive task, if you read notable durable artifacts that are not already indexed in artmark, briefly list them (skipping obvious throwaways) and ask which ones the user wants remembered. For each artifact the user approves, save and index it immediately using what you already read:

```text
artmark add <uri> --explicit --json-input - --json
```

### Locator safety

Do not save temporary or credential-bearing locators such as signed URLs, authentication callbacks, temporary download URLs, session URLs, or paths to obviously temporary files. Prefer a stable canonical locator without secrets when one is available.

The CLI rejects recognizable URL credentials, authentication parameters, and signed download parameters, including nested and encoded forms. Validation has bounded input and decoding limits; rejected registrations are not saved. Use a stable locator without secrets. Ordinary query parameters remain part of a generic web artifact's identity. This check cannot recognize arbitrary secrets or every provider's custom authentication scheme; see `docs/credential-validation.md` for the policy.

## Inspect with the existing agent harness

When saving an artifact, use the best available tool for the source to build a useful search card.

For example, use an available Drive tool for Google Drive artifacts, Figma tooling for Figma artifacts, GitHub tooling for GitHub objects, filesystem tools for local files, and web/browser tooling for ordinary web artifacts.

artmark itself should not be expected to authenticate to those systems.

Inspect only enough of the source to understand what the artifact is and make it discoverable later. Reuse source content already loaded during the current task rather than fetching it again.

Do not let artmark indexing interfere with completing the user's primary task.

## Create a retrieval-oriented search card

Create a compact search card designed to help a future agent find the artifact when the user remembers it only vaguely.

The goal is **rediscovery**, not comprehensive summarization.

Capture the artifact's purpose, important topics, project or product names, decisions or questions, relevant technologies, meaningful entities, problems it helps solve, and terminology that the user might reasonably search for later.

Include useful synonyms or alternate terminology when they improve retrieval, but do not invent unsupported facts.

Keep source-provided facts in `source_metadata`. Keep agent-generated interpretation in `catalog`.

Save a new or approved artifact with its card in one step:

```text
artmark add <uri> --explicit --json-input - --json
```

Or update an existing entry's card by ID or URI:

```text
artmark index <id> --json-input - --json
```

using stdin, or provide an equivalent temporary JSON file.

The input shape is:

```json
{
  "source_metadata": {
    "title": "Source title",
    "revision": "optional source revision",
    "updated_at": "optional source timestamp"
  },
  "catalog": {
    "summary": "Short description of the artifact",
    "search_text": "Retrieval-oriented description containing concepts and likely future search terms",
    "topics": ["topic"],
    "entities": ["project or notable entity"],
    "tags": []
  },
  "retrieval": {
    "provider": "provider name",
    "external_id": "provider object ID"
  }
}
```

`summary` and `search_text` are required.

Include retrieval fields only when known.

`search_text` should answer the question:

> What might the user vaguely remember about this artifact months from now?

For a small artifact, a short card is sufficient. For a large artifact, capture the concepts that distinguish it and are most likely to matter in future searches.

Do not copy substantial source sections into the card.

Do not store passwords, access tokens, cookies, API keys, signed URLs, authentication material, or unnecessary sensitive source data.

Avoid transient session context such as "the file we are working on right now." Prefer durable descriptions such as project names, topics, technologies, decisions, and artifact purpose.

When the user or a live source confirms that one artifact supersedes another, keep the artifact's purpose first in `summary` and add one brief sentence naming its direct predecessor or successor, with the related artmark ID when known. If both artifacts are registered, update both cards so a search result for either points to the other. Read each existing card with `artmark get` before reindexing and preserve its unrelated catalog and source metadata; `artmark index` replaces the catalog card. Keep only the direct relationship, revising stale wording as versions change rather than appending a history. A version-like title alone does not prove supersession. Do not invent a special `tags` convention for this relationship.

## Find artifacts by description

Whenever completing the user's task requires identifying an existing durable artifact from a description, search artmark before searching the provider. This includes tasks to review or update the artifact, whether or not the user explicitly says "find" or says they have seen it before, and even when they name a provider, person, topic, or recent event. If the user supplies the exact locator, register it under the policy above and use that locator directly. Before asking the user to resend a previously supplied artifact, search artmark:

```text
artmark search <query> --json
```

Use known provider, kind, topic, or other filters when they materially improve the search.

Inspect likely candidates with:

```text
artmark get <id> --json
```

If one result clearly matches, use its locator and retrieval hints to retrieve the live artifact through the appropriate provider tool.

If a search card names a related artifact needed for the task, inspect that card with `artmark get` too, then retrieve its live source as needed.

If several results remain genuinely ambiguous, use their catalog cards to narrow the choice and ask the user only if necessary.

If artmark has no plausible result, continue with other appropriate search methods or ask the user for the artifact when necessary.

## Use the live source

Once artmark identifies an artifact, retrieve its current contents from the authoritative provider when the task depends on what the artifact actually says or contains.

For example:

```text
artmark search
    ↓
find gdrive:doc:ABC
    ↓
Google Drive tool
    ↓
current Google Doc
```

Do not answer substantive questions about current source contents solely from `summary` or `search_text`.

The artmark card may be stale. Its purpose is to locate the source.

If the source is inaccessible, explain that artmark found the artifact's pointer but the current provider could not be accessed.

## Refresh opportunistically

When an already-saved artifact is revisited and live retrieval shows that it has changed enough that its existing catalog card would make future retrieval misleading or incomplete, regenerate the search card and run:

```text
artmark index <id> --json-input - --json
```

If you need to find previously saved pointer-only entries that still lack search cards, list them with `artmark recent --state registered --json`.

Do not refresh metadata merely because a timestamp or minor source detail changed.

artmark does not require background synchronization.

## Failure behavior

artmark is persistence infrastructure, not the user's primary task.

If registration or indexing fails, report the failure briefly when persistence matters and continue the user's main work whenever possible.

Never claim that an artifact was saved, indexed, updated, or retrieved unless the corresponding artmark operation succeeded.
