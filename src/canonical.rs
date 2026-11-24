use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};
use url::Url;

pub const CANONICAL_KEY_PREFIXES: &[&str] = &[
    "gdrive:",
    "figma:",
    "github:",
    "notion:",
    "slack:",
    "jira:",
    "linear:",
    "confluence:",
    "web:",
    "file:",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub canonical_key: String,
    pub kind: String,
    pub provider: Option<String>,
    pub external_id: Option<String>,
    pub uri: String,
}

pub fn identify(input: &str) -> Result<Identity> {
    identify_internal(input, None)
}

pub fn identify_with_base(input: &str, base_dir: &Path) -> Result<Identity> {
    identify_internal(input, Some(base_dir))
}

fn identify_internal(input: &str, base_dir: Option<&Path>) -> Result<Identity> {
    crate::credentials::validate(input)?;
    let input = input.trim();
    if input.is_empty() {
        bail!("artifact URI cannot be empty");
    }
    if input.starts_with("//") || is_windows_path(input) {
        return local_path(Path::new(input), input, base_dir);
    }
    let mut url = match Url::parse(input) {
        Ok(url) => url,
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            return local_path(Path::new(input), input, base_dir);
        }
        Err(error) => return Err(error.into()),
    };
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
        return local_path(&path, input, base_dir);
    }
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only HTTP(S) URLs and local paths are supported");
    }
    url.set_fragment(None);
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let mut parts: Vec<_> = url.path_segments().map(|p| p.collect()).unwrap_or_default();
    while parts.last().is_some_and(|segment| segment.is_empty()) {
        parts.pop();
    }

    let known = identify_google(&host, &parts, &url, input)
        .or_else(|| identify_figma(&host, &parts, input))
        .or_else(|| identify_github(&host, &parts, input))
        .or_else(|| identify_notion(&host, &parts, &url, input))
        .or_else(|| identify_slack(&host, &parts, &url, input))
        .or_else(|| identify_jira(&host, &parts, &url, input))
        .or_else(|| identify_linear(&host, &parts, input))
        .or_else(|| identify_confluence(&host, &parts, &url, input));

    if let Some(identity) = known.filter(|id| !id.external_id.as_deref().unwrap_or("").is_empty()) {
        return Ok(identity);
    }

    // Unknown URLs retain their query string because it may identify the object.
    // Normalizing through Url handles host case, default ports, and fragments.
    let normalized = url.to_string();
    Ok(Identity {
        canonical_key: format!("web:{normalized}"),
        kind: "web_page".into(),
        provider: Some("web".into()),
        external_id: None,
        uri: input.into(),
    })
}

fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, value)| key == name && !value.is_empty())
        .map(|(_, value)| value.into_owned())
}

fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

fn identify_google(host: &str, parts: &[&str], url: &Url, input: &str) -> Option<Identity> {
    // Strip optional Google Workspace domain prefix: /a/<domain>/...
    let parts = if parts.len() >= 3 && parts[0] == "a" && !parts[1].is_empty() {
        &parts[2..]
    } else {
        parts
    };

    if host == "docs.google.com" && parts.len() >= 3 {
        let (type_key, kind) = match parts[0] {
            "document" => ("doc", "google_doc"),
            "spreadsheets" => ("sheet", "google_sheet"),
            "presentation" => ("slide", "google_slide"),
            _ => return None,
        };
        let id = google_d_id(&parts[1..])?;
        return Some(known_identity(
            format!("gdrive:{type_key}:{id}"),
            kind,
            "google_drive",
            id,
            input,
        ));
    }

    if host == "drive.google.com" {
        if parts.first() == Some(&"file")
            && let Some(id) = google_d_id(&parts[1..])
        {
            return Some(known_identity(
                format!("gdrive:file:{id}"),
                "google_drive_file",
                "google_drive",
                id,
                input,
            ));
        }
        if parts.first() == Some(&"drive") {
            let folder_id = match &parts[1..] {
                ["folders", id, ..] if !id.is_empty() => Some(*id),
                ["u", account, "folders", id, ..] if is_digits(account) && !id.is_empty() => {
                    Some(*id)
                }
                _ => None,
            };
            if let Some(id) = folder_id {
                return Some(known_identity(
                    format!("gdrive:folder:{id}"),
                    "google_drive_folder",
                    "google_drive",
                    id,
                    input,
                ));
            }
        }
        if matches!(parts, ["open"] | ["uc"])
            && let Some(id) = query_param(url, "id")
        {
            return Some(known_identity(
                format!("gdrive:file:{id}"),
                "google_drive_file",
                "google_drive",
                &id,
                input,
            ));
        }
    }

    None
}

fn google_d_id<'a>(tail: &'a [&'a str]) -> Option<&'a str> {
    let id = match tail {
        ["d", id, ..] => *id,
        ["u", account, "d", id, ..] if is_digits(account) => *id,
        _ => return None,
    };
    // Avoid colliding Google "publish to web" links (/d/e/2PACX-...) under id "e".
    (!id.is_empty() && id != "e").then_some(id)
}

fn identify_figma(host: &str, parts: &[&str], input: &str) -> Option<Identity> {
    if !matches!(host, "www.figma.com" | "figma.com") {
        return None;
    }
    if parts.len() >= 2
        && matches!(
            parts[0],
            "file" | "design" | "board" | "proto" | "slides" | "deck"
        )
        && !parts[1].is_empty()
    {
        let file_key = if parts.len() >= 4 && parts[2] == "branch" && !parts[3].is_empty() {
            parts[3]
        } else {
            parts[1]
        };
        return Some(known_identity(
            format!("figma:file:{file_key}"),
            "figma_file",
            "figma",
            file_key,
            input,
        ));
    }
    None
}

fn identify_github(host: &str, parts: &[&str], input: &str) -> Option<Identity> {
    if !matches!(host, "github.com" | "www.github.com") || parts.len() < 2 {
        return None;
    }
    let owner = parts[0].to_ascii_lowercase();
    let raw_repo = parts[1].to_ascii_lowercase();
    if owner.is_empty()
        || raw_repo.is_empty()
        || matches!(
            owner.as_str(),
            "about"
                | "apps"
                | "collections"
                | "enterprise"
                | "events"
                | "explore"
                | "features"
                | "issues"
                | "join"
                | "login"
                | "marketplace"
                | "new"
                | "notifications"
                | "organizations"
                | "orgs"
                | "pricing"
                | "pulls"
                | "search"
                | "security"
                | "settings"
                | "sponsors"
                | "topics"
                | "users"
        )
    {
        return None;
    }

    match parts.get(2).copied() {
        None => {
            let repo_name = raw_repo
                .strip_suffix(".git")
                .filter(|stem| !stem.is_empty())
                .unwrap_or(&raw_repo);
            let repo = format!("{owner}/{repo_name}");
            Some(known_identity(
                format!("github:repo:{repo}"),
                "github_repo",
                "github",
                &repo,
                input,
            ))
        }
        Some("issues") => {
            if parts.len() == 4 && parts[3].parse::<u64>().is_ok() {
                let repo = format!("{owner}/{raw_repo}");
                Some(known_identity(
                    format!("github:issue:{repo}:{}", parts[3]),
                    "github_issue",
                    "github",
                    parts[3],
                    input,
                ))
            } else {
                None
            }
        }
        Some("pull") => {
            let valid_pr_tail = parts.len() == 4
                || (parts.len() == 5
                    && matches!(
                        parts[4],
                        "files" | "commits" | "checks" | "changes" | "conflicts"
                    ))
                || (parts.len() == 6 && parts[4] == "commits" && !parts[5].is_empty());
            if valid_pr_tail && parts[3].parse::<u64>().is_ok() {
                let repo = format!("{owner}/{raw_repo}");
                Some(known_identity(
                    format!("github:pr:{repo}:{}", parts[3]),
                    "github_pr",
                    "github",
                    parts[3],
                    input,
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn identify_notion(host: &str, parts: &[&str], url: &Url, input: &str) -> Option<Identity> {
    if !(matches!(host, "notion.so" | "www.notion.so") || host.ends_with(".notion.site")) {
        return None;
    }
    let id = query_param(url, "p")
        .and_then(|value| notion_id(&value))
        .or_else(|| parts.last().and_then(|part| notion_id(part)))?;
    Some(known_identity(
        format!("notion:page:{id}"),
        "notion_page",
        "notion",
        &id,
        input,
    ))
}

fn identify_slack(host: &str, parts: &[&str], url: &Url, input: &str) -> Option<Identity> {
    if !host.ends_with(".slack.com") || matches!(host, "app.slack.com" | "www.slack.com") {
        return None;
    }
    if parts.len() == 3
        && parts[0] == "archives"
        && !parts[1].is_empty()
        && is_slack_message_id(parts[2])
    {
        let channel = parts[1].to_ascii_uppercase();
        let thread_id = query_param(url, "thread_ts")
            .and_then(|ts| slack_thread_ts_to_id(&ts))
            .unwrap_or_else(|| parts[2].to_owned());
        return Some(known_identity(
            format!("slack:thread:{host}:{channel}:{thread_id}"),
            "slack_thread",
            "slack",
            &thread_id,
            input,
        ));
    }
    None
}

fn is_slack_message_id(value: &str) -> bool {
    value
        .strip_prefix('p')
        .is_some_and(|digits| digits.len() >= 10 && digits.chars().all(|c| c.is_ascii_digit()))
}

fn slack_thread_ts_to_id(ts: &str) -> Option<String> {
    let (seconds, fraction) = ts.split_once('.')?;
    if seconds.len() >= 10 && !fraction.is_empty() && is_digits(seconds) && is_digits(fraction) {
        Some(format!("p{seconds}{fraction}"))
    } else {
        None
    }
}

fn host_with_port(url: &Url, host: &str) -> String {
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

fn identify_jira(host: &str, parts: &[&str], url: &Url, input: &str) -> Option<Identity> {
    let is_atlassian = host.ends_with(".atlassian.net");
    let is_jira_host = is_atlassian || host.starts_with("jira.") || host.contains(".jira.");

    let raw_key: Option<String> = match parts {
        ["jira", "browse", key] => Some((*key).to_owned()),
        ["browse", key] if is_jira_host => Some((*key).to_owned()),
        ["issues", key] if is_jira_host => Some((*key).to_owned()),
        _ if is_jira_host
            && parts
                .first()
                .is_some_and(|first| matches!(*first, "jira" | "projects"))
            && parts.len() >= 3
            && parts[parts.len() - 2] == "issues" =>
        {
            Some(parts[parts.len() - 1].to_owned())
        }
        _ if is_jira_host
            && parts
                .first()
                .is_some_and(|first| matches!(*first, "jira" | "secure" | "projects")) =>
        {
            query_param(url, "selectedIssue")
        }
        _ => None,
    };

    let key = raw_key.and_then(|raw| normalize_issue_key(&raw, 2))?;
    let instance = host_with_port(url, host);
    Some(known_identity(
        format!("jira:issue:{instance}:{key}"),
        "jira_issue",
        "jira",
        &key,
        input,
    ))
}

fn identify_linear(host: &str, parts: &[&str], input: &str) -> Option<Identity> {
    if !matches!(host, "linear.app" | "www.linear.app") {
        return None;
    }
    if matches!(parts.len(), 3 | 4) && !parts[0].is_empty() && parts[1] == "issue" {
        let workspace = parts[0].to_ascii_lowercase();
        let key = normalize_issue_key(parts[2], 1)?;
        return Some(known_identity(
            format!("linear:issue:{workspace}:{key}"),
            "linear_issue",
            "linear",
            &key,
            input,
        ));
    }
    None
}

fn normalize_issue_key(raw: &str, min_project_len: usize) -> Option<String> {
    let (project, number) = raw.split_once('-')?;
    if number.contains('-') {
        return None;
    }
    if project.len() < min_project_len || project.len() > 32 {
        return None;
    }
    let mut chars = project.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    if !is_digits(number) || !number.parse::<u64>().is_ok_and(|n| n > 0) {
        return None;
    }
    Some(format!("{}-{number}", project.to_ascii_uppercase()))
}

fn identify_confluence(host: &str, parts: &[&str], url: &Url, input: &str) -> Option<Identity> {
    let is_atlassian = host.ends_with(".atlassian.net");
    let is_confluence_host =
        is_atlassian || host.starts_with("confluence.") || host.contains(".confluence.");

    let (rest, trusted_confluence_context) = if is_atlassian {
        (parts.strip_prefix(&["wiki"])?, true)
    } else if let Some(after) = parts.strip_prefix(&["confluence"]) {
        (after, true)
    } else if is_confluence_host {
        let after = parts.strip_prefix(&["wiki"]).unwrap_or(parts);
        (after, true)
    } else if let Some(after) = parts.strip_prefix(&["wiki"]) {
        (after, false)
    } else {
        (parts, false)
    };

    let page_id: Option<String> = match rest {
        ["spaces", space, "pages", id] | ["spaces", space, "pages", id, _]
            if trusted_confluence_context && !space.is_empty() && valid_numeric_id(id) =>
        {
            Some((*id).to_owned())
        }
        ["spaces", space, "pages", "edit-v2" | "edit", id]
            if trusted_confluence_context && !space.is_empty() && valid_numeric_id(id) =>
        {
            Some((*id).to_owned())
        }
        ["pages", "viewpage.action"] => {
            query_param(url, "pageId").filter(|id| valid_numeric_id(id))
        }
        _ => None,
    };

    let page_id = page_id?;
    let instance = host_with_port(url, host);
    Some(known_identity(
        format!("confluence:page:{instance}:{page_id}"),
        "confluence_page",
        "confluence",
        &page_id,
        input,
    ))
}

fn valid_numeric_id(value: &str) -> bool {
    is_digits(value) && value.parse::<u64>().is_ok_and(|n| n > 0)
}

fn notion_id(segment: &str) -> Option<String> {
    let plain = segment.rsplit('-').next()?;
    if plain.len() == 32 && plain.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(plain.to_ascii_lowercase());
    }
    let start = segment.len().checked_sub(36)?;
    if !segment.is_char_boundary(start) {
        return None;
    }
    if start > 0 && segment.as_bytes()[start - 1] != b'-' {
        return None;
    }
    let suffix = &segment[start..];
    if is_uuid_36(suffix) {
        Some(suffix.replace('-', "").to_ascii_lowercase())
    } else {
        None
    }
}

fn is_uuid_36(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn known_identity(key: String, kind: &str, provider: &str, id: &str, uri: &str) -> Identity {
    Identity {
        canonical_key: key,
        kind: kind.into(),
        provider: Some(provider.into()),
        external_id: Some(id.into()),
        uri: uri.into(),
    }
}

pub fn is_locator_or_key(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || query.starts_with("art_")
        || CANONICAL_KEY_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        || is_filesystem_path_locator(query)
}

pub fn is_filesystem_path_locator(query: &str) -> bool {
    if query.starts_with('/')
        || query.starts_with('\\')
        || query.starts_with("./")
        || query.starts_with("../")
        || query.starts_with(".\\")
        || query.starts_with("..\\")
        || query.starts_with("~/")
        || query.starts_with("~\\")
        || is_windows_path(query)
    {
        return true;
    }
    !query.chars().any(char::is_whitespace) && (query.contains('/') || query.contains('\\'))
}

fn is_windows_path(input: &str) -> bool {
    let bytes = input.as_bytes();
    let is_drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    is_drive_path || input.starts_with('\\')
}

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(raw) = path.to_str()
        && let Some(stripped) = raw.strip_prefix(r"\\?\")
    {
        let bytes = stripped.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            return PathBuf::from(stripped);
        }
    }
    path
}

fn local_path(path: &Path, original: &str, base_dir: Option<&Path>) -> Result<Identity> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base) = base_dir {
        base.join(path)
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut clean = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            _ => clean.push(part.as_os_str()),
        }
    }
    let clean = match std::fs::canonicalize(&clean) {
        Ok(resolved) => strip_windows_verbatim_prefix(resolved),
        Err(_) => clean,
    };
    let path = clean.to_string_lossy().into_owned();
    Ok(Identity {
        canonical_key: format!("file:{path}"),
        kind: if clean.is_dir() {
            "local_directory"
        } else {
            "local_file"
        }
        .into(),
        provider: Some("filesystem".into()),
        external_id: Some(path),
        uri: original.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::{identify, identify_with_base, is_filesystem_path_locator, is_locator_or_key};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn google_doc_views_share_identity() {
        let edit = identify("https://docs.google.com/document/d/ABC/edit").unwrap();
        let view = identify("https://docs.google.com/document/d/ABC/view").unwrap();
        assert_eq!(edit.canonical_key, view.canonical_key);
        assert_eq!(edit.canonical_key, "gdrive:doc:ABC");
    }

    #[test]
    fn github_issue_and_pr_are_distinct() {
        let issue = identify("https://github.com/Foo/Bar/issues/42").unwrap();
        let pr = identify("https://github.com/foo/bar/pull/42").unwrap();
        assert_eq!(issue.canonical_key, "github:issue:foo/bar:42");
        assert_eq!(pr.canonical_key, "github:pr:foo/bar:42");
    }

    #[test]
    fn generic_url_loses_fragment_but_keeps_query() {
        let a = identify("https://EXAMPLE.com/foo?q=1#top").unwrap();
        let b = identify("https://example.com/foo?q=1#bottom").unwrap();
        assert_eq!(a.canonical_key, b.canonical_key);
        assert!(a.canonical_key.contains("?q=1"));
    }

    #[test]
    fn plain_fragment_anchors_are_not_credentials() {
        let plain = identify("https://example.com/docs").unwrap();
        for anchor in ["code", "token", "signature", "auth"] {
            let with_anchor = identify(&format!("https://example.com/docs#{anchor}")).unwrap();
            assert_eq!(with_anchor.canonical_key, plain.canonical_key);
        }
    }

    #[test]
    fn safe_double_slash_paths_remain_local_paths() {
        for path in ["//localhost/artmark-missing/file", "///tmp/artmark-file"] {
            let identity = identify(path).unwrap();
            assert_eq!(identity.provider.as_deref(), Some("filesystem"));
        }
    }

    #[test]
    fn generic_code_query_identifies_distinct_artifacts() {
        let first = identify("https://example.com/products?code=ABC").unwrap();
        let same = identify("https://EXAMPLE.com/products?code=ABC#details").unwrap();
        let second = identify("https://example.com/products?code=XYZ").unwrap();
        assert_eq!(first.canonical_key, same.canonical_key);
        assert_ne!(first.canonical_key, second.canonical_key);
        assert!(identify("https://example.com/callback?code=secret").is_err());
        assert!(identify("https://example.com/products?code=ABC&state=CA").is_ok());
    }

    #[test]
    fn credential_parameters_and_userinfo_are_rejected() {
        for uri in [
            "https://example.com/file?token=secret",
            "https://example.com/file?access%5Ftoken=secret",
            "https://example.com/file?API-Key=secret",
            "https://example.com/file?X-Amz-Signature=secret&X-Amz-Expires=300",
            "https://example.com/file?X-Goog-Credential=secret",
            "https://example.com/file?GoogleAccessId=user&Signature=secret",
            "https://example.com/file?sv=1&sig=secret",
            "https://example.com/callback?code=secret&state=abc",
            "https://example.com/redirect?next=https%3A%2F%2Fexample.org%2Ffile%3Ftoken%3Dsecret",
            "https://example.com/redirect?next=%2Fdownload%3Ftoken%3Dsecret",
            "https://example.com/redirect?next=%2F%2Fexample.org%2Ffile%3Fsig%3Dsecret",
            "https://example.com/redirect?next=download%3Ftoken%3Dsecret",
            "https://example.com/redirect?next=https%253A%252F%252Fexample.org%252Ffile%253Ftoken%253Dsecret",
            "https://example.com/file#access_token=secret",
            "https://example.com/file#section?session_id=secret",
            "https://example.com/file#next=https%3A%2F%2Fexample.org%2Ffile%3Ftoken%3Dsecret",
            "https://example.com/file#next=%2Fdownload%3Ftoken%3Dsecret",
            "https://example.com/file#next=https://user:secret@example.org/file?view=1",
            "https://example.com/file#https://user:secret@example.org/file",
            "https://proxy.example/https://user:secret@example.org/file",
            "https://proxy.example/https%3A%2F%2Fuser%3Asecret%40example.org%2Ffile",
            "archive/https://user:secret@example.org/file",
            "https://user:secret@example.com/file",
            "//user:secret@example.com/file",
            "file:///tmp/report?token=secret",
            "https://docs.google.com/document/d/ABC/edit?token=secret",
        ] {
            let error = identify(uri).unwrap_err().to_string();
            assert!(!error.contains("secret"), "{error}");
        }
    }

    #[test]
    fn notion_page_title_does_not_change_identity() {
        let a =
            identify("https://www.notion.so/Planning-0123456789abcdef0123456789abcdef").unwrap();
        let b = identify("https://example.notion.site/New-Title-0123456789abcdef0123456789abcdef")
            .unwrap();
        assert_eq!(a.canonical_key, b.canonical_key);
        assert_eq!(
            a.canonical_key,
            "notion:page:0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn bare_relative_file_path_is_supported() {
        let identity = identify("README.md").unwrap();
        let expected = std::env::current_dir().unwrap().join("README.md");
        assert_eq!(
            identity.canonical_key,
            format!("file:{}", expected.display())
        );
        assert_eq!(identity.kind, "local_file");
    }

    #[test]
    fn notion_non_ascii_segments_do_not_panic() {
        let with_uuid =
            identify("https://www.notion.so/日本語-01234567-89ab-cdef-0123-456789abcdef").unwrap();
        assert_eq!(
            with_uuid.canonical_key,
            "notion:page:0123456789abcdef0123456789abcdef"
        );
        let without_id = identify("https://www.notion.so/a🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀").unwrap();
        assert_eq!(without_id.provider.as_deref(), Some("web"));
    }

    #[test]
    fn jira_linear_and_confluence_rules_are_local_and_scoped() {
        let jira_browse = identify("https://acme.atlassian.net/browse/eng-123").unwrap();
        let jira_board = identify(
            "https://acme.atlassian.net/jira/software/c/projects/ENG/boards/4?selectedIssue=ENG-123",
        )
        .unwrap();
        assert_eq!(jira_browse.canonical_key, jira_board.canonical_key);
        assert_eq!(
            jira_browse.canonical_key,
            "jira:issue:acme.atlassian.net:ENG-123"
        );
        assert_eq!(jira_browse.provider.as_deref(), Some("jira"));
        assert_eq!(jira_browse.kind, "jira_issue");
        assert_eq!(jira_browse.external_id.as_deref(), Some("ENG-123"));

        let linear_bare = identify("https://linear.app/acme/issue/ENG-42").unwrap();
        let linear_slug =
            identify("https://www.linear.app/ACME/issue/eng-42/fix-retry-storm#comment-1").unwrap();
        assert_eq!(linear_bare.canonical_key, linear_slug.canonical_key);
        assert_eq!(linear_bare.canonical_key, "linear:issue:acme:ENG-42");

        let conf_page =
            identify("https://acme.atlassian.net/wiki/spaces/ENG/pages/123456789/Overview")
                .unwrap();
        let conf_edit =
            identify("https://acme.atlassian.net/wiki/spaces/ENG/pages/edit-v2/123456789").unwrap();
        let conf_legacy =
            identify("https://acme.atlassian.net/wiki/pages/viewpage.action?pageId=123456789")
                .unwrap();
        assert_eq!(conf_page.canonical_key, conf_edit.canonical_key);
        assert_eq!(conf_page.canonical_key, conf_legacy.canonical_key);
        assert_eq!(
            conf_page.canonical_key,
            "confluence:page:acme.atlassian.net:123456789"
        );
    }

    #[test]
    fn windows_drive_paths_are_treated_as_filesystem_not_url_schemes() {
        for path in [
            r"C:\Users\dev\notes.md",
            "C:/Users/dev/notes.md",
            r"\\server\share\notes.md",
        ] {
            let identity = identify(path).unwrap();
            assert_eq!(identity.provider.as_deref(), Some("filesystem"), "{path}");
            assert_eq!(identity.kind, "local_file", "{path}");
        }
    }

    #[test]
    fn locator_and_key_classifiers_cover_all_prefixes_paths_and_prose() {
        for locator in [
            "https://example.com/doc",
            "http://example.com/doc",
            "file:///tmp/doc.md",
            "art_01950000000070008000000000000001",
            "gdrive:doc:ABC",
            "figma:file:XYZ",
            "github:repo:foo/bar",
            "notion:page:0123456789abcdef0123456789abcdef",
            "slack:thread:acme.slack.com:C123:p1700000000123456",
            "jira:issue:acme.atlassian.net:ENG-1",
            "linear:issue:acme:ENG-2",
            "confluence:page:acme.atlassian.net:12345",
            "web:https://example.com/doc",
            "file:/tmp/doc.md",
            "/tmp/file.md",
            "./docs/design.md",
            "../README.md",
            "docs/design.md",
            r"C:\Users\dev\notes.md",
            r"\\server\share\notes.md",
        ] {
            assert!(is_locator_or_key(locator), "expected locator: {locator}");
        }

        assert!(is_filesystem_path_locator("/tmp/file.md"));
        assert!(is_filesystem_path_locator("./docs/design.md"));
        assert!(is_filesystem_path_locator("../README.md"));
        assert!(is_filesystem_path_locator("docs/design.md"));
        assert!(is_filesystem_path_locator(r"C:\Users\dev\notes.md"));
        assert!(is_filesystem_path_locator(r"\\server\share\notes.md"));
        assert!(!is_filesystem_path_locator("roadmap"));
        assert!(!is_filesystem_path_locator("light/dark theme"));
        assert!(!is_locator_or_key("roadmap"));
        assert!(!is_locator_or_key("light/dark theme"));
    }

    #[test]
    fn supported_providers_produce_recognized_canonical_keys() {
        for uri in [
            "https://docs.google.com/document/d/ABC/edit",
            "https://www.figma.com/design/FILEKEY/Title",
            "https://github.com/org/repo/pull/42",
            "https://www.notion.so/Title-0123456789abcdef0123456789abcdef",
            "https://acme.slack.com/archives/C01234567/p1700000000123456",
            "https://acme.atlassian.net/browse/ENG-123",
            "https://linear.app/acme/issue/ENG-42",
            "https://acme.atlassian.net/wiki/spaces/ENG/pages/123456789/Overview",
            "https://example.com/article",
            "/tmp/local-note.md",
        ] {
            let identity = identify(uri).unwrap();
            assert!(
                is_locator_or_key(&identity.canonical_key),
                "unrecognized key {} for {uri}",
                identity.canonical_key
            );
        }
    }

    #[test]
    fn identify_with_base_resolves_relative_paths_deterministically() {
        let root = std::env::temp_dir().join(format!("artmark-canon-base-{}", Uuid::now_v7()));
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        let spec = root.join("spec.md");
        fs::write(&spec, "# Spec\n").unwrap();

        let from_root_bare = identify_with_base("spec.md", &root).unwrap();
        let from_root_dot = identify_with_base("./spec.md", &root).unwrap();
        let from_root_parent = identify_with_base("docs/../spec.md", &root).unwrap();
        let from_docs = identify_with_base("../spec.md", &docs).unwrap();
        let from_abs = identify_with_base(spec.to_str().unwrap(), &docs).unwrap();

        assert_eq!(from_root_bare.canonical_key, from_root_dot.canonical_key);
        assert_eq!(from_root_bare.canonical_key, from_root_parent.canonical_key);
        assert_eq!(from_root_bare.canonical_key, from_docs.canonical_key);
        assert_eq!(from_root_bare.canonical_key, from_abs.canonical_key);
        assert_eq!(from_root_bare.kind, "local_file");

        fs::remove_dir_all(root).unwrap();
    }
}
