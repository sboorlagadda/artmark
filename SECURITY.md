# Security policy

## Private vulnerability reports

Please report suspected vulnerabilities privately through [GitHub's **Report a vulnerability** form](https://github.com/sboorlagadda/artmark/security/advisories/new). Include the affected version, a concise reproduction, the impact, and any suggested fix. Do not put exploit details, secrets, or private artifact information in a public issue or discussion. The maintainers will acknowledge the report, investigate it, and coordinate disclosure and a fix with you.

If the reporting form is unavailable, do not open a public issue with the details. The form will be enabled when the repository is made public.

## Data boundary

Artmark stores a local SQLite catalog of artifact pointers and search metadata, including titles, summaries, topics, `search_text`, and retrieval hints. It does **not** store provider credentials or copies of source documents, and it does not fetch source material itself. The agent's existing provider tools access live sources.

Catalog metadata can still reveal sensitive names or topics. Review search cards before indexing confidential artifacts, avoid secrets and signed URLs, and protect and back up the database as private data. The default path is `$HOME/.artmark/artmark.db` on macOS/Linux and `%USERPROFILE%\.artmark\artmark.db` on Windows. On Unix, Artmark creates the default directory with `0700` permissions and the database with `0600` permissions.

## Supported versions

Artmark is an early `0.x` project. Security fixes target the latest released version; older versions are not maintained separately. Schemas and CLI behavior may change before `1.0`.
