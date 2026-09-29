---
name: artmark
description: Use the local artmark CLI to find and remember durable sources. When a task requires locating a shared file, message, design, diagram, ticket, PR, or similar source from a description, search artmark before available provider tools even if the provider is named. Also apply when the user supplies a durable locator or asks to save one; quick-register it and index only after inspecting the source. Ask at task end before registering useful sources found during research or linked from a user-supplied source.
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

## Register artifacts the user provides

Automatically quick-register durable artifact locators supplied directly by the user during the conversation.

Examples include Google Drive documents, Figma files, GitHub objects, Notion pages, stable web pages, PDFs, and durable local files.

Quick registration is intentionally cheap and does not require fetching the artifact:

```text
artmark add <uri> --json
```

If the user explicitly asks to save, remember, bookmark, or artmark the artifact, mark that intent:

```text
artmark add <uri> --explicit --json
```

Keep the returned `art_...` ID for possible indexing.

Do not register temporary or credential-bearing locators such as signed URLs, authentication callbacks, temporary download URLs, session URLs, or paths to obviously temporary files. Prefer a stable canonical locator without secrets when one is available.

The CLI rejects recognizable URL credentials, authentication parameters, and signed download parameters, including nested and encoded forms. Validation has bounded input and decoding limits; rejected registrations are not saved. Use a stable locator without secrets. Ordinary query parameters remain part of a generic web artifact's identity. This check cannot recognize arbitrary secrets or every provider's custom authentication scheme; see `docs/credential-validation.md` for the policy.

Do not register artifacts found through web or provider searches, repository exploration, or other agent research without the user's approval. Durable artifacts linked from a user-supplied artifact also need approval: the user supplied the parent, not every link inside it.

Finish the main task before asking about discovered artifacts. At the end, briefly list task-relevant durable artifacts you read, plus useful durable links found inside a user-supplied artifact, and ask which ones the user wants remembered. Do not list every search result or interrupt ordinary research for each link. If the user approves one, register it; index it if its contents were already read or the user explicitly asks to save it and it can be inspected.

## Decide when to index

Registration and indexing are different operations.

A user-provided artifact that is merely mentioned can remain quick-registered.

For a user-provided artifact, or one the user approved for registration, index it when either of these is true:

1. The user explicitly asked to save or remember it and the source can be inspected.
2. The artifact is already being read or materially used for the current task.

Do not perform expensive provider retrieval solely to enrich every incidental link.

If the artifact's contents are already available from work being performed in the current task, reuse that information rather than fetching it again.

If provider access is unavailable, keep the quick registration. Do not invent catalog metadata.

## Inspect with the existing agent harness

When indexing is appropriate, use the best available tool for the source.

For example, use an available Drive tool for Google Drive artifacts, Figma tooling for Figma artifacts, GitHub tooling for GitHub objects, filesystem tools for local files, and web/browser tooling for ordinary web artifacts.

artmark itself should not be expected to authenticate to those systems.

Inspect only enough of the source to understand what the artifact is and make it discoverable later.

Do not let artmark indexing interfere with completing the user's primary task.

## Create a retrieval-oriented search card

Create a compact search card designed to help a future agent find the artifact when the user remembers it only vaguely.

The goal is **rediscovery**, not comprehensive summarization.

Capture the artifact's purpose, important topics, project or product names, decisions or questions, relevant technologies, meaningful entities, problems it helps solve, and terminology that the user might reasonably search for later.

Include useful synonyms or alternate terminology when they improve retrieval, but do not invent unsupported facts.

Keep source-provided facts in `source_metadata`. Keep agent-generated interpretation in `catalog`.

Send the card with:

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

When a user-provided artifact appears again, normal registration may update aliases and last-seen information while preserving its canonical identity.

If live retrieval shows that the artifact has changed enough that its existing catalog card would make future retrieval misleading or incomplete, regenerate the search card and run:

```text
artmark index <id> --json-input - --json
```

Do not refresh metadata merely because a timestamp or minor source detail changed.

artmark does not require background synchronization.

## Failure behavior

artmark is persistence infrastructure, not the user's primary task.

If registration or indexing fails, report the failure briefly when persistence matters and continue the user's main work whenever possible.

Never claim that an artifact was saved, indexed, updated, or retrieved unless the corresponding artmark operation succeeded.
