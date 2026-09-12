//! AEAD seal/open and obfuscated block headers.

use anyhow::{Result, bail};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate},
};
use zeroize::Zeroize;

use crate::keys::{AAD, SubKeys};

pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const HEADER_LEN: usize = 8;
pub const HEADER_TAG_LEN: usize = 3;
const WIRE_VERSION: u8 = 1;

/// Seal payload into capsule: nonce(24) || ciphertext+tag.
///
/// When `deterministic` is true, the nonce is derived from the AEAD key and
/// plaintext (keyed BLAKE3) so identical inputs yield identical capsules.
pub fn seal(subkeys: &SubKeys, plaintext: &[u8], deterministic: bool) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(&subkeys.aead)
        .map_err(|e| anyhow::anyhow!("invalid aead key: {e}"))?;
    let nonce = if deterministic {
        deterministic_nonce(subkeys, plaintext)
    } else {
        XNonce::generate()
    };
    let ct = cipher
        .encrypt(
            &nonce,
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad: AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ct);
    Ok(out)
}

fn deterministic_nonce(subkeys: &SubKeys, plaintext: &[u8]) -> XNonce {
    let nonce_key = blake3::derive_key(
        "codestego 2026-08-10 deterministic nonce v1",
        &subkeys.aead,
    );
    let hash = blake3::Hasher::new_keyed(&nonce_key)
        .update(plaintext)
        .finalize();
    let mut bytes = [0u8; NONCE_LEN];
    bytes.copy_from_slice(&hash.as_bytes()[..NONCE_LEN]);
    XNonce::from(bytes)
}

/// Open capsule: nonce(24) || ciphertext+tag → plaintext.
pub fn open(subkeys: &SubKeys, capsule: &[u8]) -> Result<Vec<u8>> {
    if capsule.len() < NONCE_LEN + TAG_LEN {
        bail!("capsule too short");
    }
    let cipher = XChaCha20Poly1305::new_from_slice(&subkeys.aead)
        .map_err(|e| anyhow::anyhow!("invalid aead key: {e}"))?;
    let nonce = XNonce::try_from(&capsule[..NONCE_LEN])
        .map_err(|_| anyhow::anyhow!("invalid nonce length"))?;
    let pt = cipher
        .decrypt(
            &nonce,
            chacha20poly1305::aead::Payload {
                msg: &capsule[NONCE_LEN..],
                aad: AAD,
            },
        )
        .map_err(|_| anyhow::anyhow!("decryption/authentication failed"))?;
    Ok(pt)
}

/// Build an obfuscated 8-byte block header.
/// Cleartext layout: [version, shard_index, k, m, shard_size] || tag[3]
/// Then XOR with a key-derived pad.
pub fn encode_header(
    subkeys: &SubKeys,
    shard_index: u8,
    k: u8,
    m: u8,
    shard_size: u8,
) -> [u8; HEADER_LEN] {
    let mut clear = [0u8; HEADER_LEN];
    clear[0] = WIRE_VERSION;
    clear[1] = shard_index;
    clear[2] = k;
    clear[3] = m;
    clear[4] = shard_size;
    let tag = header_tag(subkeys, &clear[..5]);
    clear[5..8].copy_from_slice(&tag);

    let pad = header_pad_fixed(subkeys);
    for i in 0..HEADER_LEN {
        clear[i] ^= pad[i];
    }
    clear
}

/// Try to decode an obfuscated header. Returns (shard_index, k, m, shard_size) on success.
pub fn decode_header(
    subkeys: &SubKeys,
    obfuscated: &[u8; HEADER_LEN],
) -> Option<(u8, u8, u8, u8)> {
    let pad = header_pad_fixed(subkeys);
    let mut clear = [0u8; HEADER_LEN];
    for i in 0..HEADER_LEN {
        clear[i] = obfuscated[i] ^ pad[i];
    }
    if clear[0] != WIRE_VERSION {
        return None;
    }
    let expected = header_tag(subkeys, &clear[..5]);
    if clear[5..8] != expected {
        return None;
    }
    let shard_index = clear[1];
    let k = clear[2];
    let m = clear[3];
    let shard_size = clear[4];
    if k == 0 || shard_size == 0 {
        return None;
    }
    if (k as u16) + (m as u16) > 255 {
        return None;
    }
    Some((shard_index, k, m, shard_size))
}

fn header_tag(subkeys: &SubKeys, data: &[u8]) -> [u8; HEADER_TAG_LEN] {
    let mut hasher = blake3::Hasher::new_keyed(&subkeys.hdr);
    hasher.update(b"codestego-hdr-tag-v1");
    hasher.update(data);
    let hash = hasher.finalize();
    let mut tag = [0u8; HEADER_TAG_LEN];
    tag.copy_from_slice(&hash.as_bytes()[..HEADER_TAG_LEN]);
    tag
}

fn header_pad_fixed(subkeys: &SubKeys) -> [u8; HEADER_LEN] {
    let mut hasher = blake3::Hasher::new_keyed(&subkeys.hdr);
    hasher.update(b"codestego-hdr-pad-v1");
    let hash = hasher.finalize();
    let mut pad = [0u8; HEADER_LEN];
    pad.copy_from_slice(&hash.as_bytes()[..HEADER_LEN]);
    pad
}

/// MAC over journal record bytes.
pub fn journal_mac(key: &[u8; 32], record: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(b"codestego-journal-v1");
    hasher.update(record);
    *hasher.finalize().as_bytes()
}

/// Securely wipe a byte buffer.
pub fn wipe(buf: &mut [u8]) {
    buf.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::MasterKey;

    #[test]
    fn aead_roundtrip() {
        let sk = MasterKey::generate().derive_subkeys();
        let pt = b"hello watermark";
        let cap = seal(&sk, pt, false).unwrap();
        let out = open(&sk, &cap).unwrap();
        assert_eq!(out, pt);
    }

    #[test]
    fn wrong_key_fails() {
        let sk1 = MasterKey::generate().derive_subkeys();
        let sk2 = MasterKey::generate().derive_subkeys();
        let cap = seal(&sk1, b"secret", false).unwrap();
        assert!(open(&sk2, &cap).is_err());
    }

    #[test]
    fn deterministic_seal_identical() {
        let sk = MasterKey::generate().derive_subkeys();
        let pt = b"same plaintext";
        let a = seal(&sk, pt, true).unwrap();
        let b = seal(&sk, pt, true).unwrap();
        assert_eq!(a, b);
        assert_eq!(open(&sk, &a).unwrap(), pt);
    }

    #[test]
    fn deterministic_seal_different_plaintext_different_nonce() {
        let sk = MasterKey::generate().derive_subkeys();
        let a = seal(&sk, b"plain-a", true).unwrap();
        let b = seal(&sk, b"plain-b", true).unwrap();
        assert_ne!(&a[..NONCE_LEN], &b[..NONCE_LEN]);
    }

    #[test]
    fn random_seals_differ() {
        let sk = MasterKey::generate().derive_subkeys();
        let a = seal(&sk, b"hello", false).unwrap();
        let b = seal(&sk, b"hello", false).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn header_roundtrip() {
        let sk = MasterKey::generate().derive_subkeys();
        let enc = encode_header(&sk, 2, 3, 3, 24);
        let (idx, k, m, s) = decode_header(&sk, &enc).unwrap();
        assert_eq!((idx, k, m, s), (2, 3, 3, 24));
    }

    #[test]
    fn header_wrong_key() {
        let sk1 = MasterKey::generate().derive_subkeys();
        let sk2 = MasterKey::generate().derive_subkeys();
        let enc = encode_header(&sk1, 0, 1, 1, 24);
        assert!(decode_header(&sk2, &enc).is_none());
    }
}
