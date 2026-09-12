//! Key generation, loading, and domain-separated subkey derivation.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use argon2::{
    Argon2, PasswordHasher,
    password_hash::SaltString,
};
use rand::RngExt;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const MASTER_KEY_LEN: usize = 32;
pub const AAD: &[u8] = b"codestego/v1";

const KEY_FILE_MAGIC: &[u8; 8] = b"CSTGKEY1";
const KEY_FILE_RAW: u8 = 0;
const KEY_FILE_PASSPHRASE: u8 = 1;

/// Master key material, zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey([u8; MASTER_KEY_LEN]);

impl MasterKey {
    pub fn from_bytes(bytes: [u8; MASTER_KEY_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; MASTER_KEY_LEN] {
        &self.0
    }

    pub fn generate() -> Self {
        let mut bytes = [0u8; MASTER_KEY_LEN];
        rand::rng().fill(&mut bytes);
        Self(bytes)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s.trim()).context("invalid hex key")?;
        if bytes.len() != MASTER_KEY_LEN {
            bail!(
                "key must be {} bytes (got {})",
                MASTER_KEY_LEN,
                bytes.len()
            );
        }
        let mut arr = [0u8; MASTER_KEY_LEN];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Derive AEAD and header subkeys via blake3 domain separation.
    pub fn derive_subkeys(&self) -> SubKeys {
        let mut aead = [0u8; 32];
        let mut hdr = [0u8; 32];
        aead.copy_from_slice(
            blake3::derive_key("codestego 2026-08-10 aead key v1", &self.0).as_slice(),
        );
        hdr.copy_from_slice(
            blake3::derive_key("codestego 2026-08-10 header key v1", &self.0).as_slice(),
        );
        SubKeys { aead, hdr }
    }

    /// Journal HMAC key.
    pub fn journal_key(&self) -> [u8; 32] {
        blake3::derive_key("codestego 2026-08-10 journal hmac v1", &self.0)
    }
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SubKeys {
    pub aead: [u8; 32],
    pub hdr: [u8; 32],
}

/// Default key path: ~/.config/codestego/key
pub fn default_key_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config/codestego/key")
    } else {
        PathBuf::from("codestego.key")
    }
}

/// Generate and write a raw random key file (mode 0600).
pub fn keygen_raw(path: &Path) -> Result<MasterKey> {
    let key = MasterKey::generate();
    write_key_file(path, KEY_FILE_RAW, &key.0, &[])?;
    Ok(key)
}

/// Generate a passphrase-protected key file.
pub fn keygen_passphrase(path: &Path, passphrase: &str) -> Result<()> {
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| anyhow::anyhow!("salt encode: {e}"))?;
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(passphrase.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?;

    // Store PHC string (includes salt); derived key is never written.
    let phc = hash.to_string();
    write_key_file(path, KEY_FILE_PASSPHRASE, &[], phc.as_bytes())?;
    Ok(())
}

fn write_key_file(path: &Path, kind: u8, raw_key: &[u8], extra: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create key directory")?;
        // Restrict directory to owner
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    if path.exists() {
        bail!("key file already exists: {}", path.display());
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create key file {}", path.display()))?;
    f.write_all(KEY_FILE_MAGIC)?;
    f.write_all(&[kind])?;
    if kind == KEY_FILE_RAW {
        f.write_all(raw_key)?;
    } else {
        let len = extra.len() as u16;
        f.write_all(&len.to_le_bytes())?;
        f.write_all(extra)?;
    }
    f.sync_all()?;
    Ok(())
}

/// Load a master key from a file or refuse if permissions are too open.
pub fn load_key_file(path: &Path, passphrase: Option<&str>) -> Result<MasterKey> {
    check_key_permissions(path)?;
    let mut f = File::open(path).with_context(|| format!("open key {}", path.display()))?;
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if &magic != KEY_FILE_MAGIC {
        // Legacy: raw 32-byte key file
        if magic.len() == 8 {
            let mut rest = Vec::new();
            f.read_to_end(&mut rest)?;
            let mut all = magic.to_vec();
            all.extend_from_slice(&rest);
            if all.len() == MASTER_KEY_LEN {
                let mut arr = [0u8; MASTER_KEY_LEN];
                arr.copy_from_slice(&all);
                return Ok(MasterKey::from_bytes(arr));
            }
        }
        bail!("unrecognized key file format");
    }
    let mut kind = [0u8; 1];
    f.read_exact(&mut kind)?;
    match kind[0] {
        KEY_FILE_RAW => {
            let mut key = [0u8; MASTER_KEY_LEN];
            f.read_exact(&mut key)?;
            Ok(MasterKey::from_bytes(key))
        }
        KEY_FILE_PASSPHRASE => {
            let pass = passphrase.context("passphrase required for this key file")?;
            let mut len_buf = [0u8; 2];
            f.read_exact(&mut len_buf)?;
            let len = u16::from_le_bytes(len_buf) as usize;
            let mut phc_bytes = vec![0u8; len];
            f.read_exact(&mut phc_bytes)?;
            let phc = std::str::from_utf8(&phc_bytes).context("invalid PHC string")?;
            let parsed = argon2::PasswordHash::new(phc)
                .map_err(|e| anyhow::anyhow!("parse PHC: {e}"))?;
            let salt = parsed
                .salt
                .ok_or_else(|| anyhow::anyhow!("missing salt"))?;
            let mut salt_buf = [0u8; 64];
            let salt_raw = salt
                .decode_b64(&mut salt_buf)
                .map_err(|e| anyhow::anyhow!("salt decode: {e}"))?;
            let argon2 = Argon2::default();
            let mut key = [0u8; MASTER_KEY_LEN];
            argon2
                .hash_password_into(pass.as_bytes(), salt_raw, &mut key)
                .map_err(|e| anyhow::anyhow!("argon2 derive: {e}"))?;
            Ok(MasterKey::from_bytes(key))
        }
        other => bail!("unknown key file type: {other}"),
    }
}

fn check_key_permissions(path: &Path) -> Result<()> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mode = meta.permissions().mode() & 0o777;
    // Refuse if group or other have any permissions
    if mode & 0o077 != 0 {
        bail!(
            "refusing to load key {}: mode {:03o} is too open (must be 0600 or stricter)",
            path.display(),
            mode
        );
    }
    Ok(())
}

/// Resolve key from --key path, CODESTEGO_KEY env (hex), or default path.
pub fn resolve_key(
    key_path: Option<&Path>,
    passphrase: Option<&str>,
) -> Result<MasterKey> {
    if let Ok(hex_key) = std::env::var("CODESTEGO_KEY") {
        if !hex_key.is_empty() && key_path.is_none() {
            return MasterKey::from_hex(&hex_key);
        }
    }
    let path = key_path
        .map(PathBuf::from)
        .unwrap_or_else(default_key_path);
    load_key_file(&path, passphrase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_raw_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("key");
        let k1 = keygen_raw(&path).unwrap();
        let k2 = load_key_file(&path, None).unwrap();
        assert_eq!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn refuse_open_permissions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("key");
        let _ = keygen_raw(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_key_file(&path, None).is_err());
    }

    #[test]
    fn subkeys_differ() {
        let k = MasterKey::generate();
        let s = k.derive_subkeys();
        assert_ne!(s.aead, s.hdr);
    }

    #[test]
    fn passphrase_key_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("key");
        keygen_passphrase(&path, "correct horse battery").unwrap();
        let loaded = load_key_file(&path, Some("correct horse battery")).unwrap();
        assert_eq!(loaded.as_bytes().len(), MASTER_KEY_LEN);
        // Wrong passphrase still loads a file but derives different key material.
        let wrong = load_key_file(&path, Some("wrong-passphrase")).unwrap();
        assert_ne!(wrong.as_bytes(), loaded.as_bytes());
        assert!(load_key_file(&path, None).is_err());
    }

    #[test]
    fn resolve_key_from_env_hex() {
        let k = MasterKey::generate();
        let hex = hex::encode(k.as_bytes());
        // SAFETY: test-only, single-threaded env mutation for this case.
        unsafe {
            std::env::set_var("CODESTEGO_KEY", &hex);
        }
        let got = resolve_key(None, None).unwrap();
        unsafe {
            std::env::remove_var("CODESTEGO_KEY");
        }
        assert_eq!(got.as_bytes(), k.as_bytes());
    }
}
