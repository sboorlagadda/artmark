use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub canonical_key: String,
    pub kind: String,
    pub provider: Option<String>,
    pub external_id: Option<String>,
    pub uri: String,
}

pub fn identify(input: &str) -> Result<Identity> {
    let input = input.trim();
    if input.is_empty() {
        bail!("artifact URI cannot be empty");
    }
    let mut url = match Url::parse(input) {
        Ok(url) => url,
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            return local_path(Path::new(input), input);
        }
        Err(error) => return Err(error.into()),
    };
    if url.scheme() == "file" {
        let path = url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file URI"))?;
        return local_path(&path, input);
    }
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only HTTP(S) URLs and local paths are supported");
    }
    url.set_fragment(None);
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let parts: Vec<_> = url.path_segments().map(|p| p.collect()).unwrap_or_default();
    let field = |i: usize| parts.get(i).copied().unwrap_or("");

    let known = if host == "docs.google.com" && parts.len() >= 3 {
        let subtype = match (field(0), field(1)) {
            ("document", "d") => Some(("doc", "google_doc")),
            ("spreadsheets", "d") => Some(("sheet", "google_sheet")),
            ("presentation", "d") => Some(("slide", "google_slide")),
            _ => None,
        };
        subtype.map(|(type_key, kind)| {
            known_identity(
                format!("gdrive:{type_key}:{}", field(2)),
                kind,
                "google_drive",
                field(2),
                input,
            )
        })
    } else if host == "drive.google.com" && field(0) == "file" && field(1) == "d" {
        Some(known_identity(
            format!("gdrive:file:{}", field(2)),
            "google_drive_file",
            "google_drive",
            field(2),
            input,
        ))
    } else if host == "www.figma.com" || host == "figma.com" {
        if matches!(field(0), "file" | "design" | "board") && !field(1).is_empty() {
            Some(known_identity(
                format!("figma:file:{}", field(1)),
                "figma_file",
                "figma",
                field(1),
                input,
            ))
        } else {
            None
        }
    } else if host == "github.com" && !field(0).is_empty() && !field(1).is_empty() {
        let repo = format!("{}/{}", field(0).to_lowercase(), field(1).to_lowercase());
        match (field(2), field(3)) {
            ("issues", number) if number.parse::<u64>().is_ok() => Some(known_identity(
                format!("github:issue:{repo}:{number}"),
                "github_issue",
                "github",
                number,
                input,
            )),
            ("pull", number) if number.parse::<u64>().is_ok() => Some(known_identity(
                format!("github:pr:{repo}:{number}"),
                "github_pr",
                "github",
                number,
                input,
            )),
            ("", "") => Some(known_identity(
                format!("github:repo:{repo}"),
                "github_repo",
                "github",
                &repo,
                input,
            )),
            _ => None,
        }
    } else if matches!(host.as_str(), "notion.so" | "www.notion.so")
        || host.ends_with(".notion.site")
    {
        parts.last().and_then(|part| notion_id(part)).map(|id| {
            known_identity(
                format!("notion:page:{id}"),
                "notion_page",
                "notion",
                &id,
                input,
            )
        })
    } else if host.ends_with(".slack.com")
        && field(0) == "archives"
        && !field(1).is_empty()
        && field(2).starts_with('p')
    {
        Some(known_identity(
            format!("slack:thread:{}:{}:{}", host, field(1), field(2)),
            "slack_thread",
            "slack",
            field(2),
            input,
        ))
    } else {
        None
    };

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

fn notion_id(segment: &str) -> Option<String> {
    let plain = segment.rsplit('-').next()?;
    let suffix = if plain.len() == 32 {
        plain
    } else {
        segment.get(segment.len().checked_sub(36)?..)?
    };
    let normalized = suffix.replace('-', "").to_ascii_lowercase();
    (normalized.len() == 32 && normalized.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some(normalized)
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

fn local_path(path: &Path, original: &str) -> Result<Identity> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
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
    use super::identify;

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
        let identity = identify("spec.md").unwrap();
        let expected = std::env::current_dir().unwrap().join("spec.md");
        assert_eq!(
            identity.canonical_key,
            format!("file:{}", expected.display())
        );
        assert_eq!(identity.kind, "local_file");
    }
}
