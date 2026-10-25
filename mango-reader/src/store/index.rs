//! Store index: `repo.json` and the protobuf `index.pb` it points to.
//!
//! Field numbers are from the Mihon store format; see docs/features/01-extension-host.md.
//! Fields the core does not use (mirrors, messages, contact links) are left out and skipped.

use crate::error::{AppError, Result};
use prost::Message;
use serde::Deserialize;
use std::io::Read;

/// Cap on an unpacked index, against a hostile or broken store.
const MAX_UNPACKED: u64 = 128 << 20;

#[derive(Deserialize)]
pub struct RepoJson {
    pub index_v2: Option<String>,
    #[serde(default)]
    pub meta: RepoMeta,
}

#[derive(Deserialize, Default)]
pub struct RepoMeta {
    #[serde(rename = "signingKeyFingerprint")]
    pub signing_key: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StoreMsg {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub badge_label: String,
    #[prost(string, tag = "3")]
    pub signing_key: String,
    #[prost(message, optional, tag = "4")]
    pub contact: Option<ContactMsg>,
    #[prost(message, optional, tag = "101")]
    pub extension_list: Option<ExtensionListMsg>,
    #[prost(string, tag = "102")]
    pub extension_list_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ContactMsg {
    #[prost(string, tag = "1")]
    pub website: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ExtensionListMsg {
    #[prost(message, repeated, tag = "1")]
    pub extensions: Vec<ExtensionMsg>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ExtensionMsg {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub package_name: String,
    #[prost(message, optional, tag = "3")]
    pub resources: Option<ResourcesMsg>,
    #[prost(string, tag = "4")]
    pub extension_lib: String,
    #[prost(int64, tag = "5")]
    pub version_code: i64,
    #[prost(string, tag = "6")]
    pub version_name: String,
    /// 0 or 1 safe, 2 mixed, 3 NSFW.
    #[prost(int32, tag = "7")]
    pub content_warning: i32,
    #[prost(message, repeated, tag = "8")]
    pub sources: Vec<SourceMsg>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResourcesMsg {
    #[prost(string, tag = "1")]
    pub apk_url: String,
    #[prost(string, tag = "2")]
    pub icon_url: String,
    #[prost(string, tag = "501")]
    pub jar_url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct SourceMsg {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub language: String,
    #[prost(string, tag = "4")]
    pub home_url: String,
}

impl StoreMsg {
    pub fn extensions(&self) -> &[ExtensionMsg] {
        self.extension_list.as_ref().map(|list| list.extensions.as_slice()).unwrap_or(&[])
    }
}

/// Decodes an `index.pb`. Stores publish it gzipped; a plain one is read too.
pub fn parse(data: &[u8]) -> Result<StoreMsg> {
    StoreMsg::decode(unpack(data)?.as_slice()).map_err(unreadable)
}

/// Decodes the extension list a store may keep in a file of its own.
pub fn parse_list(data: &[u8]) -> Result<ExtensionListMsg> {
    ExtensionListMsg::decode(unpack(data)?.as_slice()).map_err(unreadable)
}

/// A certificate fingerprint as 64 lower-case hex digits, or None when it is not one.
pub fn normalize_key(key: &str) -> Option<String> {
    let hex: String = key.chars().filter(|c| !matches!(c, ':' | ' ')).collect::<String>().to_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

fn unpack(data: &[u8]) -> Result<Vec<u8>> {
    if !data.starts_with(&[0x1f, 0x8b]) {
        return Ok(data.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut out)
        .map_err(|e| AppError::new(502, "bad_index", format!("The repository index is damaged: {e}")))?;
    if out.len() as u64 > MAX_UNPACKED {
        return Err(AppError::new(502, "bad_index", "The repository index is too large"));
    }
    Ok(out)
}

fn unreadable(e: prost::DecodeError) -> AppError {
    AppError::new(502, "bad_index", format!("This does not look like a repository index: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample() -> StoreMsg {
        StoreMsg {
            name: "Example".into(),
            signing_key: "AB".repeat(32),
            contact: Some(ContactMsg { website: "https://example.org".into() }),
            extension_list: Some(ExtensionListMsg {
                extensions: vec![ExtensionMsg {
                    name: "Reader".into(),
                    package_name: "example.reader".into(),
                    resources: Some(ResourcesMsg { jar_url: "https://example.org/reader.jar".into(), ..Default::default() }),
                    version_code: 106004,
                    content_warning: 3,
                    sources: vec![SourceMsg { id: -42, name: "Reader".into(), language: "en".into(), ..Default::default() }],
                    ..Default::default()
                }],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn plain_and_gzipped() {
        let plain = sample().encode_to_vec();
        assert_eq!(parse(&plain).unwrap(), sample());

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&plain).unwrap();
        let parsed = parse(&gz.finish().unwrap()).unwrap();
        assert_eq!(parsed.extensions()[0].sources[0].id, -42);
        assert_eq!(parsed.extensions()[0].resources.as_ref().unwrap().jar_url, "https://example.org/reader.jar");
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"<html>not found</html>").is_err());
    }

    #[test]
    fn keys() {
        assert_eq!(normalize_key(&"AB".repeat(32)), Some("ab".repeat(32)));
        assert_eq!(normalize_key(&["AB"; 32].join(":")), Some("ab".repeat(32)));
        assert_eq!(normalize_key("abcd"), None);
        assert_eq!(normalize_key(""), None);
    }
}
