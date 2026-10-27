//! The two small encodings MPDb.tv's request signing needs: SHA-1 and base64url.
//!
//! The service derives the subscription key it expects in every request path from
//! `sha1(username + api key + subscription key)` and identifies the account by its name encoded
//! as base64url. Neither is used for anything that needs to resist an attacker (the digest is the
//! service's own request signature), so these are plain implementations checked against the
//! published test vectors.

/// The SHA-1 digest of `data` as 40 lower-case hex digits.
pub(super) fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    // Padding: a 1 bit, zeros up to 56 mod 64, then the bit length as 64 bits.
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *state = state.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `data` as base64url (RFC 4648, section 5) with `=` padding.
pub(super) fn base64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let group = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                let index = (group >> (18 - 6 * i)) & 0x3F;
                out.push(char::from(ALPHABET[index as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_published_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // Longer than one block, and exactly at the padding boundary.
        assert_eq!(
            sha1_hex(&[b'a'; 1_000]),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
        assert_eq!(sha1_hex(&[b'a'; 55]).len(), 40);
        assert_eq!(sha1_hex(&[b'a'; 56]).len(), 40);
        assert_eq!(sha1_hex(&[b'a'; 64]).len(), 40);
    }

    #[test]
    fn base64url_pads_and_uses_the_url_alphabet() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg==");
        assert_eq!(base64url(b"fo"), "Zm8=");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"test"), "dGVzdA==");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8=");
        assert_eq!(base64url(&[0xfb, 0xef, 0xbe]), "----");
        assert_eq!(base64url("müller".as_bytes()), "bcO8bGxlcg==");
    }
}
