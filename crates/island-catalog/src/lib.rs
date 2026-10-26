//! The signed release catalog (docs/design/modules-handoff.md §4.4): `modules.json` plus its minisign
//! signature, attached to every GitHub release. The base reads it to update itself and its modules,
//! and tools/pack-modules writes it. No platform code, so it builds and tests anywhere.
//!
//! Keys are made with `cargo tauri signer generate`. Tauri wraps minisign's text formats in one more
//! layer of base64; everything here accepts both the plain and the wrapped form.

use std::fmt;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use minisign_verify::PublicKey;

/// The catalog format this code reads and writes.
pub const SCHEMA: u32 = 1;
/// Where the base finds the newest catalog (the README's download button uses the same `latest`).
pub const CATALOG_URL: &str = "https://github.com/AbyssLotus/island-shell/releases/latest/download/modules.json";
pub const SIGNATURE_URL: &str = "https://github.com/AbyssLotus/island-shell/releases/latest/download/modules.json.minisig";
/// Biggest catalog the base will download.
pub const MAX_CATALOG_BYTES: usize = 1 << 20;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Catalog {
    pub schema: u32,
    /// RFC 3339.
    pub published: String,
    /// The base installer of this release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Base>,
    /// The oldest base the listed modules work with.
    #[serde(default)]
    pub base_min: String,
    /// Module API versions this release's base supports.
    #[serde(default)]
    pub api: Vec<u32>,
    #[serde(default)]
    pub modules: Vec<Module>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Base {
    pub version: String,
    /// The one-click installer (`IslandShellSetup.exe`).
    pub url: String,
    /// Lowercase hex.
    pub sha256: String,
    pub size: u64,
    /// One or two sentences for the update prompt.
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    pub description: String,
    #[serde(rename = "talksTo", default)]
    pub talks_to: Vec<String>,
    pub size: u64,
    pub url: String,
    /// Lowercase hex, of the zip.
    pub sha256: String,
    /// What a search for it can say before it's installed ("lyrics" finds Music).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Ticked to start with in the setup guide's module step (Music).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preselect: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The build has no public key (or it doesn't parse).
    NoKey,
    BadSignature,
    BadCatalog(String),
    /// A download didn't match the size or hash the signed catalog gave.
    Mismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoKey => write!(f, "This build of Island Shell has no update key, so it can't check for updates"),
            Error::BadSignature => write!(f, "The update catalog's signature doesn't match, so it was ignored"),
            Error::BadCatalog(why) => write!(f, "The update catalog couldn't be read: {why}"),
            Error::Mismatch => write!(f, "The download doesn't match the signed catalog, so it was thrown away"),
        }
    }
}

impl std::error::Error for Error {}

/// Undoes Tauri's extra base64 layer, if the text has one.
fn unwrap_tauri(text: &str) -> String {
    let t = text.trim();
    if t.starts_with("untrusted comment:") {
        return t.to_string();
    }
    match B64.decode(t.replace(['\r', '\n'], "")) {
        Ok(inner) => match String::from_utf8(inner) {
            Ok(s) if s.trim_start().starts_with("untrusted comment:") => s,
            _ => t.to_string(),
        },
        Err(_) => t.to_string(),
    }
}

/// Reads a public key: the bare key (`RW…`), a minisign `.pub` file, or Tauri's base64 of one.
/// Lines starting with `#` are ignored, so a key file can carry a note.
pub fn public_key(text: &str) -> Result<PublicKey, Error> {
    let body: String = text.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    let t = unwrap_tauri(&body);
    if t.is_empty() {
        return Err(Error::NoKey);
    }
    if t.starts_with("untrusted comment:") {
        PublicKey::decode(&t).map_err(|_| Error::NoKey)
    } else {
        PublicKey::from_base64(&t).map_err(|_| Error::NoKey)
    }
}

/// Checks a minisign signature (plain, or Tauri's base64 of one) over `data`.
/// Only prehashed signatures are accepted, which is what minisign and Tauri write today.
pub fn verify(data: &[u8], signature: &str, key: &PublicKey) -> Result<(), Error> {
    let sig = minisign_verify::Signature::decode(&unwrap_tauri(signature)).map_err(|_| Error::BadSignature)?;
    key.verify(data, &sig, false).map_err(|_| Error::BadSignature)
}

/// Verifies the signature first, then parses and checks the catalog.
pub fn parse_verified(data: &[u8], signature: &str, key: &PublicKey) -> Result<Catalog, Error> {
    if data.len() > MAX_CATALOG_BYTES {
        return Err(Error::BadCatalog("it's too big".into()));
    }
    verify(data, signature, key)?;
    let catalog: Catalog = serde_json::from_slice(data).map_err(|e| Error::BadCatalog(e.to_string()))?;
    check(&catalog).map_err(Error::BadCatalog)?;
    Ok(catalog)
}

/// The rules every catalog follows, signed or not (pack-modules runs this before signing).
pub fn check(c: &Catalog) -> Result<(), String> {
    if c.schema != SCHEMA {
        return Err(format!("schema {} isn't supported (this version reads {SCHEMA})", c.schema));
    }
    if let Some(b) = &c.base {
        version(&b.version).ok_or_else(|| format!("base version {:?} isn't semver", b.version))?;
        check_url(&b.url)?;
        check_hash(&b.sha256)?;
    }
    let mut seen = std::collections::HashSet::new();
    for m in &c.modules {
        if !valid_id(&m.id) {
            return Err(format!("module id {:?} isn't valid", m.id));
        }
        if !seen.insert(m.id.as_str()) {
            return Err(format!("module {} is listed twice", m.id));
        }
        version(&m.version).ok_or_else(|| format!("{} version {:?} isn't semver", m.id, m.version))?;
        check_url(&m.url)?;
        check_hash(&m.sha256)?;
    }
    Ok(())
}

fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") && !url.contains(char::is_whitespace) {
        Ok(())
    } else {
        Err(format!("{url:?} isn't an https link"))
    }
}

fn check_hash(h: &str) -> Result<(), String> {
    if h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        Ok(())
    } else {
        Err(format!("{h:?} isn't a lowercase sha256"))
    }
}

/// `^[a-z][a-z0-9-]{1,31}$` (handoff §3).
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=32).contains(&b.len()) && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

pub fn version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v')).ok()
}

/// True when `candidate` is a newer version than `current`. Anything unparsable is never newer.
pub fn newer(candidate: &str, current: &str) -> bool {
    match (version(candidate), version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// A download is only kept if it's exactly what the signed catalog promised.
pub fn check_download(data: &[u8], size: u64, sha256: &str) -> Result<(), Error> {
    if data.len() as u64 == size && sha256_hex(data) == sha256 {
        Ok(())
    } else {
        Err(Error::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisign::KeyPair;

    fn keys() -> (KeyPair, PublicKey) {
        let kp = KeyPair::generate_unencrypted_keypair().unwrap();
        let pk = public_key(&kp.pk.to_base64()).unwrap();
        (kp, pk)
    }

    fn sign(kp: &KeyPair, data: &[u8]) -> String {
        minisign::sign(Some(&kp.pk), &kp.sk, data, None, None).unwrap().to_string()
    }

    fn sample() -> Catalog {
        Catalog {
            schema: 1,
            published: "2026-11-02T18:00:00Z".into(),
            base: Some(Base {
                version: "0.2.0".into(),
                url: "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/IslandShellSetup.exe".into(),
                sha256: sha256_hex(b"installer"),
                size: 9,
                notes: "Modules.".into(),
            }),
            base_min: "0.2.0".into(),
            api: vec![1],
            modules: vec![Module {
                id: "destiny".into(),
                name: "Destiny 2".into(),
                version: "1.0.0".into(),
                api: 1,
                description: "Characters, loadouts and DIM.".into(),
                talks_to: vec!["bungie.net".into()],
                size: 3,
                url: "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/destiny-1.0.0.zip".into(),
                sha256: sha256_hex(b"zip"),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn signed_catalog_round_trips() {
        let (kp, pk) = keys();
        let data = serde_json::to_vec_pretty(&sample()).unwrap();
        let sig = sign(&kp, &data);
        assert_eq!(parse_verified(&data, &sig, &pk).unwrap(), sample());
    }

    #[test]
    fn tauri_wrapped_key_and_signature_work() {
        let (kp, _) = keys();
        let pub_file = kp.pk.to_box().unwrap().into_string();
        let wrapped_key = B64.encode(&pub_file);
        let pk = public_key(&wrapped_key).unwrap();
        assert!(public_key(&pub_file).is_ok());
        let data = b"{}";
        let wrapped_sig = B64.encode(sign(&kp, data));
        assert!(verify(data, &wrapped_sig, &pk).is_ok());
    }

    #[test]
    fn tampering_is_caught() {
        let (kp, pk) = keys();
        let data = serde_json::to_vec(&sample()).unwrap();
        let sig = sign(&kp, &data);
        let mut bad = data.clone();
        let i = bad.iter().position(|&b| b == b'2').unwrap();
        bad[i] = b'9';
        assert_eq!(parse_verified(&bad, &sig, &pk), Err(Error::BadSignature));
        let (_, other) = keys();
        assert_eq!(parse_verified(&data, &sig, &other), Err(Error::BadSignature));
        assert_eq!(verify(&data, "not a signature", &pk), Err(Error::BadSignature));
    }

    #[test]
    fn missing_or_noted_keys() {
        assert_eq!(public_key("").err(), Some(Error::NoKey));
        assert_eq!(public_key("# Put the public key from `cargo tauri signer generate` here.\n").err(), Some(Error::NoKey));
        let (kp, _) = keys();
        assert!(public_key(&format!("# note\n{}\n", kp.pk.to_base64())).is_ok());
    }

    #[test]
    fn rules() {
        let mut c = sample();
        assert!(check(&c).is_ok());
        c.schema = 2;
        assert!(check(&c).is_err());
        let mut c = sample();
        c.base.as_mut().unwrap().url = "http://example.com/x.exe".into();
        assert!(check(&c).is_err());
        let mut c = sample();
        c.modules[0].sha256 = "ABC".into();
        assert!(check(&c).is_err());
        let mut c = sample();
        c.modules.push(c.modules[0].clone());
        assert!(check(&c).is_err());
        let mut c = sample();
        c.modules[0].id = "Destiny".into();
        assert!(check(&c).is_err());
    }

    #[test]
    fn ids() {
        for ok in ["music", "wallpaper-engine", "vpn", "d2"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "m", "Music", "9lives", "-x", "a_b", "abcdefghijklmnopqrstuvwxyzabcdefg"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn versions() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "1.0.0-beta.2"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("latest", "0.1.0"));
    }

    #[test]
    fn downloads() {
        assert!(check_download(b"installer", 9, &sha256_hex(b"installer")).is_ok());
        assert_eq!(check_download(b"installer", 10, &sha256_hex(b"installer")), Err(Error::Mismatch));
        assert_eq!(check_download(b"installex", 9, &sha256_hex(b"installer")), Err(Error::Mismatch));
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
