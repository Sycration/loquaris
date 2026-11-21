//! Small helpers: random IDs and tokens, hashing, timestamps.
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the system random source failed");
    bytes
}

/// A random 16-character base-62 ID, the same form the browser generates for
/// lessons and entries.
pub fn new_id() -> String {
    let mut id = String::with_capacity(16);
    while id.len() < 16 {
        for byte in random_bytes::<32>() {
            // 248 = 4 × 62: rejecting larger bytes keeps the digits uniform.
            if byte < 248 && id.len() < 16 {
                id.push(BASE62[usize::from(byte % 62)] as char);
            }
        }
    }
    id
}

/// A 256-bit random secret (sessions, invites, reset links), hex encoded.
pub fn new_token() -> String {
    hex::encode(random_bytes::<32>())
}

/// Tokens are stored only as their SHA-256, so a leaked database does not
/// leak usable sessions or links.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Milliseconds since the Unix epoch; every timestamp column uses this.
pub fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// ISO 8601 for the API.
pub fn iso(ms: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Trimmed, lowercased email; `None` if it is not plausibly an address.
pub fn normalize_email(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let plausible = !local.is_empty()
        && !domain.is_empty()
        && email.len() <= 254
        && !email.chars().any(char::is_whitespace);
    plausible.then_some(email)
}

/// A file name derived from a project name: path separators, control
/// characters and characters Windows rejects are dropped.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"/\:*?"<>|"#.contains(*c))
        .collect();
    let stem = stem.trim().trim_matches('.').trim();
    if stem.is_empty() { "Loquaris".into() } else { stem.chars().take(120).collect() }
}

/// A `Content-Disposition: attachment` value with an ASCII fallback and the
/// exact UTF-8 name (RFC 6266), so macrons survive.
pub fn attachment(file_name: &str) -> String {
    let ascii: String = file_name
        .chars()
        .map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let mut encoded = String::new();
    for byte in file_name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_base62() {
        let id = new_id();
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|b| BASE62.contains(&b)));
        assert_ne!(new_id(), id);
    }

    #[test]
    fn emails_are_normalized() {
        assert_eq!(normalize_email("  Ann@Example.ORG "), Some("ann@example.org".into()));
        assert_eq!(normalize_email("nope"), None);
        assert_eq!(normalize_email("a b@c"), None);
    }

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(file_stem(" Lingua/Latīna: I "), "LinguaLatīna I");
        assert_eq!(file_stem("..."), "Loquaris");
        assert_eq!(
            attachment("Latīna.pdf"),
            "attachment; filename=\"Lat_na.pdf\"; filename*=UTF-8''Lat%C4%ABna.pdf"
        );
    }

    #[test]
    fn timestamps_are_iso() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_500), "1970-01-01T00:00:01.5Z");
    }
}
