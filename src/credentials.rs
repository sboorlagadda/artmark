//! Local validation of the original text, before canonicalization can discard it.
//! Policy and intentional ambiguity limits: docs/credential-validation.md.

use anyhow::{Result, bail};
use std::collections::{HashSet, VecDeque};
use url::Url;

const MAX_INPUT: usize = 64 * 1024;
const MAX_WORK: usize = 1024 * 1024;
const MAX_TASKS: usize = 1024;
const MAX_DECODES: usize = 8;
const LIMIT_ERROR: &str = "artifact locator exceeds validation limits";
const CREDENTIAL_ERROR: &str = "artifact locator contains credentials";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Kind {
    Text,
    Locator,
    Value,
    Fragment(bool),
}

#[derive(Default)]
struct Inspector {
    pending: VecDeque<(String, Kind, usize)>,
    seen: HashSet<(String, Kind)>,
    work: usize,
}

pub fn validate(input: &str) -> Result<()> {
    if input.len() > MAX_INPUT {
        bail!(LIMIT_ERROR);
    }
    let mut inspector = Inspector::default();
    inspector.push(input, Kind::Text, 0)?;
    while let Some((text, kind, decodes)) = inspector.pending.pop_front() {
        inspector.inspect(&text, kind, decodes)?;
    }
    Ok(())
}

impl Inspector {
    fn push(&mut self, text: &str, kind: Kind, decodes: usize) -> Result<()> {
        if decodes > MAX_DECODES {
            bail!(LIMIT_ERROR);
        }
        let key = (text.to_owned(), kind);
        if self.seen.contains(&key) {
            return Ok(());
        }
        self.work += text.len();
        if self.work > MAX_WORK || self.seen.len() >= MAX_TASKS {
            bail!(LIMIT_ERROR);
        }
        self.seen.insert(key.clone());
        self.pending.push_back((key.0, kind, decodes));
        Ok(())
    }

    fn inspect(&mut self, text: &str, kind: Kind, decodes: usize) -> Result<()> {
        // Keep both views. In particular, never normalize away dot segments
        // before checking the original path for embedded credential URLs.
        let decoded = percent_decode_once(text);
        if decoded != text {
            self.push(&decoded, kind, decodes + 1)?;
        }
        let url_view: String = text
            .chars()
            .filter(|ch| !matches!(ch, '\t' | '\r' | '\n'))
            .map(|ch| if ch == '\\' { '/' } else { ch })
            .collect();
        if url_view != text {
            self.push(&url_view, kind, decodes)?;
        }

        let lower = text.to_ascii_lowercase();
        for prefix in ["https:", "http:", "file:", "//"] {
            for (offset, _) in lower.match_indices(prefix) {
                // A scheme's authority is already inspected with that URL.
                if prefix == "//" && offset > 0 && text.as_bytes()[offset - 1] == b':' {
                    continue;
                }
                self.push(&text[offset..], Kind::Locator, decodes)?;
            }
        }

        let is_locator = kind == Kind::Locator
            || (kind == Kind::Value
                && (text.starts_with('/')
                    || text.starts_with('?')
                    || text.starts_with("./")
                    || text.starts_with("../")
                    || text.contains('?')));
        if is_locator || matches!(kind, Kind::Fragment(_)) {
            self.locator(text, kind, decodes)?;
        }
        Ok(())
    }

    fn locator(&mut self, text: &str, kind: Kind, decodes: usize) -> Result<()> {
        // Parsing supplements raw inspection; its normalized path is never the
        // source of the text we inspect or persist.
        let parsed = if text.starts_with("//") {
            Url::parse(&format!("https:{text}"))
        } else {
            Url::parse(text)
        };
        if parsed
            .as_ref()
            .is_ok_and(|url| !url.username().is_empty() || url.password().is_some())
        {
            bail!(CREDENTIAL_ERROR);
        }
        // Also catch recognizable userinfo in an otherwise malformed authority.
        if let Some((_, authority)) = text.split_once("//")
            && authority
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .contains('@')
        {
            bail!(CREDENTIAL_ERROR);
        }

        let (before_fragment, fragment) = text
            .split_once('#')
            .map_or((text, None), |(head, tail)| (head, Some(tail)));
        let (path, query) = before_fragment
            .split_once('?')
            .map_or((before_fragment, None), |(head, tail)| (head, Some(tail)));
        let oauth = matches!(kind, Kind::Fragment(true))
            || oauth_path(path)
            || parsed.as_ref().is_ok_and(|url| oauth_path(url.path()));
        if matches!(kind, Kind::Fragment(_)) && path.contains('=') {
            // In #name=value?tail, '?' belongs to the value. Inspect the
            // parameter itself before inspecting a nested locator in its value.
            self.parameters(before_fragment, oauth, decodes)?;
        } else if let Some(query) = query {
            self.parameters(query, oauth, decodes)?;
        }
        if let Some(fragment) = fragment {
            self.push(fragment, Kind::Fragment(oauth), decodes)?;
        }
        Ok(())
    }

    fn parameters(&mut self, text: &str, oauth: bool, decodes: usize) -> Result<()> {
        for parameter in text.split('&') {
            // Bare anchors and valueless query terms carry no credential value.
            let Some((name, value)) = parameter.split_once('=') else {
                continue;
            };
            if credential_name(name) || (oauth && name.eq_ignore_ascii_case("code")) {
                bail!(CREDENTIAL_ERROR);
            }
            self.push(value, Kind::Value, decodes)?;
        }
        Ok(())
    }
}

fn oauth_path(path: &str) -> bool {
    // Exclude the authority: a hostname named "auth" is not an auth endpoint.
    let path = if let Some((_, rest)) = path.split_once("//") {
        rest.find('/').map_or("", |offset| &rest[offset..])
    } else {
        path
    };
    let lower = path.to_ascii_lowercase();
    let endpoint = |segment: &str| {
        matches!(
            segment,
            "authorize" | "callback" | "cb" | "login" | "signin" | "auth" | "sso"
        )
    };
    let mut normalized = Vec::new();
    let mut last = "";
    for segment in lower.split('/').filter(|segment| !segment.is_empty()) {
        if matches!(segment, "oauth" | "oauth2") {
            return true;
        }
        last = segment;
        match segment {
            "." => {}
            ".." => {
                normalized.pop();
            }
            _ => normalized.push(segment),
        }
    }
    endpoint(last) || normalized.last().is_some_and(|segment| endpoint(segment))
}

fn credential_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_'))
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    normalized.starts_with("xamz")
        || normalized.starts_with("xgoog")
        || matches!(
            normalized.as_str(),
            "token"
                | "accesstoken"
                | "refreshtoken"
                | "idtoken"
                | "authtoken"
                | "oauthtoken"
                | "bearertoken"
                | "sessiontoken"
                | "apitoken"
                | "key"
                | "apikey"
                | "accesskey"
                | "accesskeyid"
                | "awsaccesskeyid"
                | "googleaccessid"
                | "resourcekey"
                | "secret"
                | "secretkey"
                | "clientsecret"
                | "password"
                | "passwd"
                | "pwd"
                | "auth"
                | "authkey"
                | "authorization"
                | "credential"
                | "signature"
                | "sig"
                | "oauthsignature"
                | "jwt"
                | "session"
                | "sessionid"
                | "samlresponse"
        )
}

fn percent_decode_once(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::identify, credential_cases};

    #[test]
    fn credential_matrix_is_rejected_before_identity_construction() {
        for uri in credential_cases::rejected() {
            let error = identify(&uri).expect_err(&uri).to_string();
            assert_eq!(error, CREDENTIAL_ERROR, "{uri}");
        }
    }

    #[test]
    fn ordinary_locators_remain_registerable() {
        for uri in credential_cases::accepted() {
            assert!(identify(&uri).is_ok(), "{uri}");
        }
    }

    #[test]
    fn repeated_safe_locators_do_not_create_recursive_cycles() {
        let mut uri = "https://example.com/products?id=1".to_owned();
        for _ in 0..16 {
            uri = format!("https://example.com/?next={uri}");
        }
        assert!(validate(&uri).is_ok());
    }

    #[test]
    fn excessive_inputs_have_a_static_limit_error() {
        let mut encoded = "%41".to_owned();
        for _ in 0..MAX_DECODES {
            encoded = encoded.replace('%', "%25");
        }
        let many_parameters = (0..MAX_TASKS)
            .map(|i| format!("item{i}=value{i}"))
            .collect::<Vec<_>>()
            .join("&");
        let repeated_urls = "https://example.com/".repeat(400);
        for uri in [
            "x".repeat(MAX_INPUT + 1),
            format!("https://example.com/?q={encoded}"),
            format!("https://example.com/?{many_parameters}"),
            format!("https://example.com/{repeated_urls}"),
        ] {
            assert_eq!(validate(&uri).unwrap_err().to_string(), LIMIT_ERROR);
        }
        assert!(validate(&"x".repeat(MAX_INPUT)).is_ok());
    }
}
