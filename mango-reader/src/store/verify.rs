//! JAR signature check: the v1 scheme that Android and jarsigner share.
//!
//! An extension passes when a signer block in META-INF holds a valid signature by
//! the pinned certificate, that signer's .SF file covers MANIFEST.MF, and the
//! manifest covers every other file in the archive. Only SHA-256 with RSA is
//! accepted; anything else is refused rather than guessed at.

use base64::{engine::general_purpose::STANDARD, Engine};
use cms::{
    cert::CertificateChoices,
    content_info::ContentInfo,
    signed_data::{SignedData, SignerInfo},
};
use der::{asn1::ObjectIdentifier, Decode, Encode};
use rsa::{
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
    RsaPublicKey,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};

const SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const SHA256_WITH_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
const MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");

const MANIFEST: &str = "META-INF/MANIFEST.MF";
/// Cap on what one archive may unpack to.
const MAX_UNPACKED: u64 = 256 << 20;

#[derive(Debug, PartialEq)]
pub enum VerifyError {
    /// There is no signature to check.
    Unsigned,
    /// Properly signed, but by other keys. Holds their fingerprints.
    WrongKey(Vec<String>),
    /// A broken archive, a broken signature, or content the signature does not cover.
    Invalid(String),
}

type Files = HashMap<String, Vec<u8>>;

/// Checks that `jar` is signed by the certificate with fingerprint `pinned` (sha256, lower-case hex).
pub fn verify_jar(jar: &[u8], pinned: &str) -> Result<(), VerifyError> {
    verify_files(&read_entries(jar)?, pinned)
}

fn verify_files(files: &Files, pinned: &str) -> Result<(), VerifyError> {
    let manifest = files.get(MANIFEST).ok_or(VerifyError::Unsigned)?;
    let mut other_keys = Vec::new();
    let mut broken = None;
    for (name, block) in files {
        let Some(base) = name.strip_suffix(".RSA").filter(|_| is_signature_file(name)) else { continue };
        let Some(sf) = files.get(&format!("{base}.SF")) else { continue };
        match signer(block, sf) {
            Ok(fingerprint) if fingerprint == pinned => {
                check_signature_file(sf, manifest)?;
                return check_manifest(manifest, files);
            }
            Ok(fingerprint) => other_keys.push(fingerprint),
            Err(why) => broken = Some(why),
        }
    }
    Err(match (other_keys.is_empty(), broken) {
        (false, _) => VerifyError::WrongKey(other_keys),
        (true, Some(why)) => VerifyError::Invalid(why),
        (true, None) => VerifyError::Unsigned,
    })
}

fn invalid(why: impl Into<String>) -> VerifyError {
    VerifyError::Invalid(why.into())
}

/// Every file in the archive, by name. Fails on anything two zip readers could see differently.
fn read_entries(jar: &[u8]) -> Result<Files, VerifyError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(jar)).map_err(|e| invalid(format!("not a zip archive: {e}")))?;
    // A reader that folds repeated names together would hide the copy the JVM might load.
    if declared_entries(jar) != Some(zip.len()) {
        return Err(invalid("the archive has repeated or hidden entries"));
    }
    let mut files = Files::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| invalid(format!("unreadable entry: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut data = Vec::new();
        (&mut entry)
            .take(MAX_UNPACKED - total + 1)
            .read_to_end(&mut data)
            .map_err(|e| invalid(format!("unreadable entry {name}: {e}")))?;
        total += data.len() as u64;
        if total > MAX_UNPACKED {
            return Err(invalid("the archive unpacks to too much data"));
        }
        if files.insert(name.clone(), data).is_some() {
            return Err(invalid(format!("the archive holds {name} twice")));
        }
    }
    Ok(files)
}

/// The entry count in the end-of-central-directory record. None when the archive ends
/// in no such record, or holds more than one place that could be it: readers do not
/// agree on which of them counts.
fn declared_entries(jar: &[u8]) -> Option<usize> {
    const RECORD: usize = 22;
    let last = jar.len().checked_sub(RECORD)?;
    let mut marks = (last.saturating_sub(0xFFFF)..=last).filter(|at| jar[*at..*at + 4] == [0x50, 0x4b, 0x05, 0x06]);
    let (at, None) = (marks.next()?, marks.next()) else { return None };
    let record = &jar[at..];
    let comment = u16::from_le_bytes([record[20], record[21]]) as usize;
    (RECORD + comment == record.len()).then(|| u16::from_le_bytes([record[10], record[11]]) as usize)
}

/// Files that carry the signature itself and so cannot be covered by it.
fn is_signature_file(name: &str) -> bool {
    let Some(file) = name.strip_prefix("META-INF/") else { return false };
    !file.contains('/') && (file == "MANIFEST.MF" || [".SF", ".RSA", ".DSA", ".EC"].iter().any(|ext| file.ends_with(ext)))
}

/// Verifies a PKCS#7 signer block against its .SF file and returns the fingerprint of the certificate that signed it.
fn signer(block: &[u8], sf: &[u8]) -> Result<String, String> {
    let info = ContentInfo::from_der(block).map_err(|e| format!("unreadable signature block: {e}"))?;
    if info.content_type != SIGNED_DATA {
        return Err("the signature block is not PKCS#7 signed data".into());
    }
    let content = info.content.to_der().map_err(|e| e.to_string())?;
    let signed = SignedData::from_der(&content).map_err(|e| format!("unreadable signature block: {e}"))?;
    let certificates = signed.certificates.as_ref().ok_or("the signature block carries no certificate")?;

    for signer in signed.signer_infos.0.iter() {
        let Some(message) = signed_message(signer, sf)? else { continue };
        let Ok(signature) = Signature::try_from(signer.signature.as_bytes()) else { continue };
        for choice in certificates.0.iter() {
            let CertificateChoices::Certificate(certificate) = choice else { continue };
            let Ok(key_der) = certificate.tbs_certificate.subject_public_key_info.to_der() else { continue };
            let Ok(key) = RsaPublicKey::from_public_key_der(&key_der) else { continue };
            if VerifyingKey::<Sha256>::new(key).verify(&message, &signature).is_ok() {
                let der = certificate.to_der().map_err(|e| e.to_string())?;
                return Ok(hex::encode(Sha256::digest(der)));
            }
        }
    }
    Err("no valid SHA-256 RSA signature over the signature file".into())
}

/// The bytes a signer signed, or None when it uses algorithms this check does not handle.
fn signed_message(signer: &SignerInfo, sf: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let algorithm = signer.signature_algorithm.oid;
    if signer.digest_alg.oid != SHA256 || (algorithm != RSA && algorithm != SHA256_WITH_RSA) {
        return Ok(None);
    }
    // Without signed attributes the signature is over the file itself. With them it is
    // over the attributes, which must then name the file's digest.
    let Some(attributes) = &signer.signed_attrs else { return Ok(Some(sf.to_vec())) };
    let digest = Sha256::digest(sf);
    let names_file =
        attributes.iter().any(|a| a.oid == MESSAGE_DIGEST && a.values.iter().any(|value| value.value() == digest.as_slice()));
    if !names_file {
        return Err("the signed attributes do not match the signature file".into());
    }
    attributes.to_der().map(Some).map_err(|e| e.to_string())
}

fn check_signature_file(sf: &[u8], manifest: &[u8]) -> Result<(), VerifyError> {
    let sections = parse_sections(sf);
    let main = sections.first().ok_or_else(|| invalid("the signature file is empty"))?;
    let digest = attribute(main, "SHA-256-Digest-Manifest")
        .ok_or_else(|| invalid("the signature file has no SHA-256 digest of the manifest"))?;
    if !digest_matches(digest, manifest) {
        return Err(invalid("MANIFEST.MF was changed after signing"));
    }
    Ok(())
}

fn check_manifest(manifest: &[u8], files: &Files) -> Result<(), VerifyError> {
    let sections = parse_sections(manifest);
    let digests: HashMap<&str, &str> =
        sections.iter().skip(1).filter_map(|s| Some((attribute(s, "Name")?, attribute(s, "SHA-256-Digest")?))).collect();
    for (name, data) in files {
        if is_signature_file(name) {
            continue;
        }
        match digests.get(name.as_str()) {
            Some(digest) if digest_matches(digest, data) => {}
            Some(_) => return Err(invalid(format!("{name} was changed after signing"))),
            None => return Err(invalid(format!("{name} is not covered by the signature"))),
        }
    }
    Ok(())
}

fn digest_matches(base64: &str, data: &[u8]) -> bool {
    STANDARD.decode(base64.trim()).is_ok_and(|want| want.as_slice() == Sha256::digest(data).as_slice())
}

fn attribute<'a>(section: &'a [(String, String)], name: &str) -> Option<&'a str> {
    section.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

/// Splits a manifest or signature file into sections of `key: value` lines.
/// A line break followed by one space continues the line before it.
fn parse_sections(data: &[u8]) -> Vec<Vec<(String, String)>> {
    let mut unfolded = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        let byte = data[at];
        at += 1;
        if byte != b'\r' && byte != b'\n' {
            unfolded.push(byte);
            continue;
        }
        if byte == b'\r' && data.get(at) == Some(&b'\n') {
            at += 1;
        }
        if data.get(at) == Some(&b' ') {
            at += 1;
        } else {
            unfolded.push(b'\n');
        }
    }

    let mut sections = Vec::new();
    let mut section = Vec::new();
    for line in String::from_utf8_lossy(&unfolded).split('\n') {
        if line.is_empty() {
            if !section.is_empty() {
                sections.push(std::mem::take(&mut section));
            }
        } else if let Some((key, value)) = line.split_once(": ") {
            section.push((key.to_owned(), value.to_owned()));
        }
    }
    if !section.is_empty() {
        sections.push(section);
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_ends_in_one_record_only() {
        let end = |entries: u16| {
            let mut record: Vec<u8> = vec![0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0];
            record.extend(entries.to_le_bytes());
            record.extend([0u8; 10]);
            record
        };
        assert_eq!(declared_entries(&end(3)), Some(3));
        assert_eq!(declared_entries(&[end(1), end(3)].concat()), None, "two readers could each go by another");
        assert_eq!(declared_entries(&[end(3), vec![0]].concat()), None, "bytes after the record that its comment does not cover");
    }

    // Built by tests/fixtures/make.py with a throwaway key.
    const PLAIN: &[u8] = include_bytes!("../../tests/fixtures/signed.jar");
    const WITH_ATTRIBUTES: &[u8] = include_bytes!("../../tests/fixtures/signed-attrs.jar");
    const CLASS: &str = "example/Main.class";

    fn key() -> &'static str {
        include_str!("../../tests/fixtures/fingerprint.txt").trim()
    }

    fn files() -> Files {
        read_entries(PLAIN).unwrap()
    }

    #[test]
    fn accepts_a_signed_jar() {
        assert_eq!(verify_jar(PLAIN, key()), Ok(()));
        assert_eq!(verify_jar(WITH_ATTRIBUTES, key()), Ok(()));
    }

    #[test]
    fn refuses_another_key() {
        assert_eq!(verify_jar(PLAIN, &"0".repeat(64)), Err(VerifyError::WrongKey(vec![key().to_owned()])));
    }

    #[test]
    fn refuses_a_changed_file() {
        let mut files = files();
        files.get_mut(CLASS).unwrap().push(0);
        assert!(matches!(verify_files(&files, key()), Err(VerifyError::Invalid(why)) if why.contains("changed after signing")));
    }

    #[test]
    fn refuses_an_added_file() {
        let mut files = files();
        files.insert("example/Extra.class".into(), b"added".to_vec());
        assert!(matches!(verify_files(&files, key()), Err(VerifyError::Invalid(why)) if why.contains("not covered")));
    }

    #[test]
    fn refuses_a_rewritten_manifest() {
        let mut files = files();
        files.insert(CLASS.into(), b"other code".to_vec());
        let manifest = String::from_utf8(files[MANIFEST].clone()).unwrap();
        let old = STANDARD.encode(Sha256::digest(&read_entries(PLAIN).unwrap()[CLASS]));
        let new = STANDARD.encode(Sha256::digest(b"other code"));
        assert!(manifest.contains(&old));
        files.insert(MANIFEST.into(), manifest.replace(&old, &new).into_bytes());
        assert_eq!(verify_files(&files, key()), Err(invalid("MANIFEST.MF was changed after signing")));
    }

    #[test]
    fn refuses_a_rewritten_signature_file() {
        for jar in [PLAIN, WITH_ATTRIBUTES] {
            let mut files = read_entries(jar).unwrap();
            files.get_mut("META-INF/CERT.SF").unwrap().extend_from_slice(b"\r\n");
            assert!(matches!(verify_files(&files, key()), Err(VerifyError::Invalid(_))));
        }
    }

    #[test]
    fn refuses_an_unsigned_jar() {
        let mut files = files();
        files.remove("META-INF/CERT.RSA");
        assert_eq!(verify_files(&files, key()), Err(VerifyError::Unsigned));
        assert!(matches!(verify_jar(b"not a zip", key()), Err(VerifyError::Invalid(_))));
    }

    #[test]
    fn counts_entries_and_unfolds_lines() {
        assert_eq!(declared_entries(PLAIN), Some(files().len()));
        let sections = parse_sections(b"Manifest-Version: 1.0\r\n\r\nName: a/very/long/na\r\n me.class\r\nSHA-256-Digest: abc\n\n");
        assert_eq!(sections.len(), 2);
        assert_eq!(attribute(&sections[1], "name"), Some("a/very/long/name.class"));
        // The fixture has a name long enough to be folded, and it still has to match.
        assert!(files().keys().any(|name| name.len() > 72));
    }
}
