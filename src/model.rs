use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub id: String,
    pub canonical_key: String,
    pub kind: String,
    pub provider: Option<String>,
    pub state: String,
    pub primary_uri: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub search_text: Option<String>,
    pub source_external_id: Option<String>,
    pub source_revision: Option<String>,
    pub source_updated_at: Option<String>,
    pub retrieval: Option<Value>,
    pub source_metadata: Option<Value>,
    pub catalog_metadata: Option<Value>,
    pub provenance: Option<Value>,
    pub is_explicitly_saved: bool,
    pub topics: Vec<String>,
    pub entities: Vec<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_seen_at: String,
    pub last_indexed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IndexInput {
    #[serde(default)]
    pub source_metadata: Option<Value>,
    pub catalog: CatalogInput,
    #[serde(default)]
    pub retrieval: Option<Value>,
    #[serde(default)]
    pub resolver: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CatalogInput {
    pub summary: String,
    pub search_text: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl IndexInput {
    pub fn validate(&self) -> Result<()> {
        if self.catalog.summary.trim().is_empty() || self.catalog.search_text.trim().is_empty() {
            bail!("catalog summary and search_text must be nonempty");
        }
        if self.catalog.summary.len() > 2_048 || self.catalog.search_text.len() > 16_384 {
            bail!("catalog text is too long for a retrieval card");
        }
        for (name, values) in [
            ("topics", &self.catalog.topics),
            ("entities", &self.catalog.entities),
            ("tags", &self.catalog.tags),
        ] {
            if values.len() > 100 || values.iter().any(|value| value.len() > 256) {
                bail!("{name} exceeds catalog limits");
            }
        }
        if let Some(metadata) = &self.source_metadata {
            validate_small_object(metadata, "source_metadata", 16_384)?;
        }
        if let Some(retrieval) = &self.retrieval {
            validate_small_object(retrieval, "retrieval", 4_096)?;
        }
        Ok(())
    }
}

fn validate_small_object(value: &Value, name: &str, max_size: usize) -> Result<()> {
    if !value.is_object() {
        bail!("{name} must be a JSON object");
    }
    if value.to_string().len() > max_size {
        bail!("{name} exceeds its size limit");
    }
    if has_forbidden_key(value) {
        bail!("{name} contains content or credential fields; provide metadata only");
    }
    Ok(())
}

fn has_forbidden_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            ["content", "body", "text", "html", "markdown"]
                .iter()
                .any(|forbidden| key == *forbidden || key.ends_with(&format!("_{forbidden}")))
                || [
                    "password",
                    "secret",
                    "token",
                    "cookie",
                    "authorization",
                    "api_key",
                    "apikey",
                ]
                .iter()
                .any(|forbidden| key.contains(forbidden))
                || has_forbidden_key(value)
        }),
        Value::Array(values) => values.iter().any(has_forbidden_key),
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchCard {
    pub id: String,
    pub canonical_key: String,
    pub title: Option<String>,
    pub kind: String,
    pub provider: Option<String>,
    pub state: String,
    pub summary: Option<String>,
    pub primary_uri: Option<String>,
    pub topics: Vec<String>,
    pub source_updated_at: Option<String>,
    pub last_indexed_at: Option<String>,
    pub score: f64,
}

impl SearchCard {
    pub fn from_artifact(artifact: &Artifact, score: f64) -> Self {
        Self {
            id: artifact.id.clone(),
            canonical_key: artifact.canonical_key.clone(),
            title: artifact.title.clone(),
            kind: artifact.kind.clone(),
            provider: artifact.provider.clone(),
            state: artifact.state.clone(),
            summary: artifact.summary.clone(),
            primary_uri: artifact.primary_uri.clone(),
            topics: artifact.topics.clone(),
            source_updated_at: artifact.source_updated_at.clone(),
            last_indexed_at: artifact.last_indexed_at.clone(),
            score,
        }
    }
}

#[derive(Default)]
pub struct SearchFilters<'a> {
    pub provider: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub topic: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub state: Option<&'a str>,
    pub created_after: Option<&'a str>,
    pub seen_after: Option<&'a str>,
    pub indexed_after: Option<&'a str>,
}

impl SearchFilters<'_> {
    pub fn allows(&self, artifact: &Artifact) -> bool {
        self.provider
            .is_none_or(|v| artifact.provider.as_deref() == Some(v))
            && self.kind.is_none_or(|v| artifact.kind == v)
            && self
                .topic
                .is_none_or(|v| artifact.topics.iter().any(|t| t.eq_ignore_ascii_case(v)))
            && self
                .tag
                .is_none_or(|v| artifact.tags.iter().any(|t| t.eq_ignore_ascii_case(v)))
            && self.state.is_none_or(|v| artifact.state == v)
            && self
                .created_after
                .is_none_or(|v| artifact.created_at.as_str() >= v)
            && self
                .seen_after
                .is_none_or(|v| artifact.last_seen_at.as_str() >= v)
            && self
                .indexed_after
                .is_none_or(|v| artifact.last_indexed_at.as_deref().is_some_and(|d| d >= v))
    }
}

#[cfg(test)]
mod tests {
    use super::IndexInput;
    use serde_json::json;

    #[test]
    fn index_rejects_source_body_and_credentials() {
        for field in ["body", "accessToken"] {
            let input: IndexInput = serde_json::from_value(json!({
                "source_metadata": {field: "should not persist"},
                "catalog": {"summary": "A document", "search_text": "A retrieval card"}
            }))
            .unwrap();
            assert!(input.validate().is_err());
        }
    }
}
