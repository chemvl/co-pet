//! Primitives shared by the account keys (M8) and the sync engine (M9): random keys, HKDF,
//! XChaCha20-Poly1305 sealing and the Crockford base32 form of the recovery key.
use chacha20poly1305::aead::inout::InOutBuf;
use chacha20poly1305::{AeadInOut, Key, KeyInit, Tag, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::{AppError, AppResult};

/// First byte of every sealed value; another value means a format this build can't read.
const VERSION: u8 = 0x01;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = 1 + NONCE_LEN;

/// Crockford base32: no `I`, `L`, `O`, `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// 256 bits in 5-bit characters, the last one padded with 4 zero bits.
const KEY_CHARS: usize = 52;
const GROUP: usize = 4;

pub fn random32() -> [u8; 32] {
    let mut buf = [0u8; 32];
    fill_random(&mut buf);
    buf
}

/// The OS generator failing leaves nothing safe to fall back to.
fn fill_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("the OS random number generator failed");
}

/// HKDF-SHA256 with a 32-byte output.
pub fn hkdf32(ikm: &[u8], salt: &[u8], info: &str) -> [u8; 32] {
    let mut okm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info.as_bytes(), &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 length");
    okm
}

/// `0x01 || 24-byte random nonce || XChaCha20-Poly1305 ciphertext || 16-byte tag`.
pub fn seal(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; NONCE_LEN];
    fill_random(&mut nonce);
    let mut out = Vec::with_capacity(HEADER_LEN + plain.len() + TAG_LEN);
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(plain);
    let tag = cipher(key)
        .encrypt_inout_detached(&XNonce::from(nonce), aad, InOutBuf::from(&mut out[HEADER_LEN..]))
        .expect("XChaCha20-Poly1305 has no practical length limit");
    out.extend_from_slice(&tag);
    out
}

/// Reverses [`seal`]. A wrong version byte, a short input or a failed tag check (wrong key,
/// wrong `aad`, altered bytes) is `decrypt`.
pub fn open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> AppResult<Vec<u8>> {
    let fail = || AppError::Code("decrypt");
    if sealed.len() < HEADER_LEN + TAG_LEN || sealed[0] != VERSION {
        return Err(fail());
    }
    let (body, tag) = sealed[HEADER_LEN..].split_at(sealed.len() - HEADER_LEN - TAG_LEN);
    let nonce = XNonce::try_from(&sealed[1..HEADER_LEN]).map_err(|_| fail())?;
    let tag = Tag::try_from(tag).map_err(|_| fail())?;
    let mut plain = body.to_vec();
    cipher(key)
        .decrypt_inout_detached(&nonce, aad, InOutBuf::from(&mut plain[..]), &tag)
        .map_err(|_| fail())?;
    Ok(plain)
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&Key::from(*key))
}

/// 52 Crockford characters in groups of 4 joined by `-` (`XXXX-XXXX-…`, 64 characters).
pub fn crockford_encode(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(KEY_CHARS + KEY_CHARS / GROUP - 1);
    let mut written = 0;
    let mut push = |v: u32| {
        if written > 0 && written % GROUP == 0 {
            out.push('-');
        }
        out.push(ALPHABET[v as usize] as char);
        written += 1;
    };
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            push((acc >> bits) & 31);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        push((acc << (5 - bits)) & 31);
    }
    out
}

/// Lenient reading of [`crockford_encode`]: any case, `-` and whitespace ignored, `O` read as
/// `0` and `I`/`L` as `1`. `None` unless exactly 52 valid characters with zero padding bits.
pub fn crockford_decode(text: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    let (mut acc, mut bits, mut chars, mut len) = (0u32, 0u32, 0usize, 0usize);
    for c in text.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = ALPHABET.iter().position(|&a| char::from(a) == c)? as u32;
        chars += 1;
        if chars > KEY_CHARS {
            return None;
        }
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out[len] = (acc >> bits) as u8;
            len += 1;
            acc &= (1 << bits) - 1;
        }
    }
    (chars == KEY_CHARS && acc == 0).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip() {
        let key = random32();
        let sealed = seal(&key, b"items\0id1", b"hello");
        assert_eq!(sealed[0], 0x01);
        assert_eq!(sealed.len(), 1 + 24 + 5 + 16);
        assert_eq!(open(&key, b"items\0id1", &sealed).unwrap(), b"hello");
        // A fresh nonce every time.
        assert_ne!(seal(&key, b"", b"x"), seal(&key, b"", b"x"));
        assert_eq!(open(&key, b"", &seal(&key, b"", b"")).unwrap(), b"");
    }

    #[test]
    fn open_rejects_aad_mismatch_and_tampering() {
        let key = random32();
        let sealed = seal(&key, b"items\0id1", b"hello");
        let code = |r: AppResult<Vec<u8>>| match r {
            Err(AppError::Code(c)) => c,
            other => panic!("expected a code, got {:?}", other.map(|_| ())),
        };
        assert_eq!(code(open(&key, b"items\0id2", &sealed)), "decrypt");
        assert_eq!(code(open(&random32(), b"items\0id1", &sealed)), "decrypt");
        let mut altered = sealed.clone();
        altered[30] ^= 1;
        assert_eq!(code(open(&key, b"items\0id1", &altered)), "decrypt");
        let mut version = sealed.clone();
        version[0] = 0x02;
        assert_eq!(code(open(&key, b"items\0id1", &version)), "decrypt");
        assert_eq!(code(open(&key, b"items\0id1", &sealed[..40])), "decrypt");
        assert_eq!(code(open(&key, b"", &[])), "decrypt");
    }

    #[test]
    fn hkdf_matches_rfc5869() {
        // RFC 5869 test case 3 (empty salt and info); the first 32 bytes of its 42-byte OKM.
        assert_eq!(
            hex::encode(hkdf32(&[0x0b; 22], b"", "")),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d"
        );
        let a = hkdf32(b"ikm", b"user", "copet recovery v1");
        assert_ne!(a, hkdf32(b"ikm", b"user", "copet records v1"));
        assert_ne!(a, hkdf32(b"ikm", b"other", "copet recovery v1"));
    }

    #[test]
    fn crockford_round_trip() {
        for bytes in [[0u8; 32], [0xff; 32], random32(), random32()] {
            let text = crockford_encode(&bytes);
            assert_eq!(text.len(), 64);
            let groups: Vec<&str> = text.split('-').collect();
            assert_eq!(groups.len(), 13);
            assert!(groups.iter().all(|g| g.len() == 4 && g.bytes().all(|c| ALPHABET.contains(&c))));
            assert_eq!(crockford_decode(&text), Some(bytes));
        }
        assert_eq!(crockford_encode(&[0u8; 32]), "0000-".repeat(12) + "0000");
        assert_eq!(&crockford_encode(&[0xff; 32])[60..], "ZZZG");
    }

    #[test]
    fn crockford_decode_is_lenient() {
        let bytes = random32();
        let text = crockford_encode(&bytes);
        assert_eq!(crockford_decode(&text.to_lowercase()), Some(bytes));
        assert_eq!(crockford_decode(&text.replace('-', "")), Some(bytes));
        assert_eq!(crockford_decode(&format!("  {}\n", text.replace('-', " "))), Some(bytes));
        let zeros_ones = "0000-".repeat(12) + "0010";
        let decoded = crockford_decode(&zeros_ones).unwrap();
        let typed = "OoOo-".repeat(12) + "0Oi0";
        assert_eq!(crockford_decode(&typed), Some(decoded));
        assert_eq!(crockford_decode(&("0000-".repeat(12) + "0Ol0")), Some(decoded));
        // Wrong length, letters outside the alphabet, non-zero padding bits.
        assert_eq!(crockford_decode(&text[..text.len() - 1]), None);
        assert_eq!(crockford_decode(&format!("{text}0")), None);
        assert_eq!(crockford_decode(&("0000-".repeat(12) + "000U")), None);
        assert_eq!(crockford_decode(&("0000-".repeat(12) + "0001")), None);
        assert_eq!(crockford_decode(""), None);
    }
}
