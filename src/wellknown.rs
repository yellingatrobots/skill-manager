//! The well-known discovery index: `/.well-known/agent-skills/index.json`.

use crate::error::{Error, Result};
use serde::Deserialize;

pub const INDEX_PATH: &str = ".well-known/agent-skills/index.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    /// As published: absolute, or relative to the endpoint.
    pub url: String,
    /// The archive digest, `sha256:<hex>`.
    pub digest: String,
}

#[derive(Debug, Deserialize)]
struct RawIndex {
    #[serde(default)]
    skills: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    name: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    url: Option<String>,
    digest: Option<String>,
}

/// Parses the index, keeping only archive entries that can actually be fetched.
pub fn parse_index(json: &str) -> Result<Vec<IndexEntry>> {
    let raw: RawIndex = serde_json::from_str(json)
        .map_err(|error| Error::Network(format!("malformed well-known index: {error}")))?;

    let mut entries = Vec::new();
    for entry in raw.skills {
        if entry.kind.as_deref().is_some_and(|kind| kind != "archive") {
            continue;
        }
        let (Some(url), Some(digest)) = (entry.url, entry.digest) else {
            return Err(Error::Network(format!(
                "well-known index entry `{}` has no url or digest",
                entry.name
            )));
        };
        if crate::tree::Digest::parse(&digest).is_none() {
            return Err(Error::Network(format!(
                "well-known index entry `{}` has an unusable digest `{digest}`",
                entry.name
            )));
        }
        entries.push(IndexEntry {
            name: entry.name,
            url,
            digest,
        });
    }
    Ok(entries)
}

pub fn index_url(endpoint: &str) -> String {
    join(endpoint, INDEX_PATH)
}

/// Resolves an index `url`, which may be absolute or endpoint-relative.
pub fn archive_url(endpoint: &str, url: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") {
        return url.to_string();
    }
    join(endpoint, url)
}

fn join(endpoint: &str, path: &str) -> String {
    format!(
        "{}/{}",
        endpoint.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIKEC4: &str = r#"{
      "$schema": "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
      "skills": [
        {
          "name": "likec4-dsl",
          "type": "archive",
          "description": "Use when working with .c4 files",
          "url": "/.well-known/agent-skills/likec4-dsl.tar.gz",
          "digest": "sha256:bb4c8e439a95f70ced963fe4055446104e76fe13c5d49e3194a4e0f10f380d92"
        }
      ]
    }"#;

    #[test]
    fn reads_the_published_index_shape() {
        let entries = parse_index(LIKEC4).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "likec4-dsl");
        assert_eq!(
            entries[0].url,
            "/.well-known/agent-skills/likec4-dsl.tar.gz"
        );
        assert!(entries[0].digest.starts_with("sha256:"));
    }

    #[test]
    fn builds_index_and_archive_urls() {
        assert_eq!(
            index_url("https://likec4.dev/"),
            "https://likec4.dev/.well-known/agent-skills/index.json"
        );
        assert_eq!(
            archive_url(
                "https://likec4.dev/",
                "/.well-known/agent-skills/likec4-dsl.tar.gz"
            ),
            "https://likec4.dev/.well-known/agent-skills/likec4-dsl.tar.gz"
        );
        assert_eq!(
            archive_url("https://likec4.dev/", "https://cdn.example.test/a.tar.gz"),
            "https://cdn.example.test/a.tar.gz"
        );
    }

    #[test]
    fn skips_entries_that_are_not_archives() {
        let json = r#"{"skills":[{"name":"x","type":"git","url":"https://x/y.git"}]}"#;
        assert!(parse_index(json).unwrap().is_empty());
    }

    #[test]
    fn rejects_entries_without_a_usable_digest() {
        let json =
            r#"{"skills":[{"name":"x","type":"archive","url":"/x.tar.gz","digest":"md5:aa"}]}"#;
        assert!(
            parse_index(json)
                .unwrap_err()
                .to_string()
                .contains("unusable digest")
        );
        let json = r#"{"skills":[{"name":"x","type":"archive","url":"/x.tar.gz"}]}"#;
        assert!(
            parse_index(json)
                .unwrap_err()
                .to_string()
                .contains("no url or digest")
        );
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(
            parse_index("not json")
                .unwrap_err()
                .to_string()
                .contains("malformed")
        );
    }
}
