//! JSONL embed journal with keyed MAC.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::crypto;
use crate::frame::Payload;

#[derive(Debug, Serialize)]
pub struct JournalRecord {
    pub path: String,
    pub original_blake3: String,
    pub watermarked_blake3: String,
    pub owner: String,
    pub recipient: String,
    pub note: String,
    pub timestamp: u32,
    pub unix_logged: u64,
    pub mac: String,
}

pub fn append(
    journal_path: &Path,
    key: &[u8; 32],
    path: &Path,
    original: &str,
    watermarked: &str,
    payload: &Payload,
) -> Result<()> {
    if let Some(parent) = journal_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let original_blake3 = blake3::hash(original.as_bytes()).to_hex().to_string();
    let watermarked_blake3 = blake3::hash(watermarked.as_bytes()).to_hex().to_string();
    let unix_logged = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // MAC over canonical fields (without mac itself)
    let mut preimage = Vec::new();
    preimage.extend_from_slice(path.to_string_lossy().as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(original_blake3.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(watermarked_blake3.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(payload.owner.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(payload.recipient.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(payload.note.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(&payload.timestamp.to_le_bytes());
    preimage.extend_from_slice(&unix_logged.to_le_bytes());

    let mac = crypto::journal_mac(key, &preimage);

    let rec = JournalRecord {
        path: path.display().to_string(),
        original_blake3,
        watermarked_blake3,
        owner: payload.owner.clone(),
        recipient: payload.recipient.clone(),
        note: payload.note.clone(),
        timestamp: payload.timestamp,
        unix_logged,
        mac: hex::encode(mac),
    };

    let line = serde_json::to_string(&rec).context("serialize journal")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path)
        .with_context(|| format!("open journal {}", journal_path.display()))?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Payload;
    use crate::keys::MasterKey;
    use tempfile::tempdir;

    #[test]
    fn append_jsonl_and_mac() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("j.jsonl");
        let master = MasterKey::generate();
        let key = master.journal_key();
        let payload = Payload::new("Acme", "bob", "n1");
        append(&path, &key, Path::new("a.rs"), "orig", "marked", &payload).unwrap();
        append(&path, &key, Path::new("b.rs"), "o2", "m2", &payload).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2);

        let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v["owner"], "Acme");
        assert_eq!(v["recipient"], "bob");
        let mac_hex = v["mac"].as_str().unwrap();

        let mut preimage = Vec::new();
        preimage.extend_from_slice(b"a.rs");
        preimage.push(0);
        preimage.extend_from_slice(v["original_blake3"].as_str().unwrap().as_bytes());
        preimage.push(0);
        preimage.extend_from_slice(v["watermarked_blake3"].as_str().unwrap().as_bytes());
        preimage.push(0);
        preimage.extend_from_slice(b"Acme");
        preimage.push(0);
        preimage.extend_from_slice(b"bob");
        preimage.push(0);
        preimage.extend_from_slice(b"n1");
        preimage.push(0);
        preimage.extend_from_slice(&payload.timestamp.to_le_bytes());
        let unix = v["unix_logged"].as_u64().unwrap();
        preimage.extend_from_slice(&unix.to_le_bytes());
        let expected = crypto::journal_mac(&key, &preimage);
        assert_eq!(hex::encode(expected), mac_hex);
    }
}
