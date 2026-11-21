//! Fractional-index order keys, compatible with the `fractional-indexing`
//! npm package the browser uses. Keys compare as plain byte strings; a key
//! can always be generated between any two others, so reordering changes one
//! field instead of shifting positions.
const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

fn digit(c: u8) -> usize {
    DIGITS
        .iter()
        .position(|&d| d == c)
        .expect("invalid order key digit")
}

fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let n = (0..)
            .take_while(|&i| i < b.len() && a.get(i).copied().unwrap_or(b'0') == b[i])
            .count();
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
            return out;
        }
    }
    let digit_a = a.first().map_or(0, |&c| digit(c));
    let digit_b = b.map_or(DIGITS.len(), |b| digit(b[0]));
    if digit_b - digit_a > 1 {
        let mid = (digit_a + digit_b).div_ceil(2);
        vec![DIGITS[mid]]
    } else if let Some(b) = b.filter(|b| b.len() > 1) {
        vec![b[0]]
    } else {
        let mut out = vec![DIGITS[digit_a]];
        out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
        out
    }
}

fn integer_length(head: u8) -> Result<usize, String> {
    match head {
        b'a'..=b'z' => Ok(usize::from(head - b'a') + 2),
        b'A'..=b'Z' => Ok(usize::from(b'Z' - head) + 2),
        _ => Err(format!("invalid order key head: {}", head as char)),
    }
}

fn integer_part(key: &str) -> Result<&str, String> {
    let head = *key.as_bytes().first().ok_or("empty order key")?;
    let length = integer_length(head)?;
    key.get(..length)
        .ok_or_else(|| format!("invalid order key: {key}"))
}

pub fn validate(key: &str) -> Result<(), String> {
    if key == SMALLEST_INTEGER {
        return Err(format!("invalid order key: {key}"));
    }
    if !key.bytes().all(|c| DIGITS.contains(&c)) {
        return Err(format!("invalid order key: {key}"));
    }
    let integer = integer_part(key)?;
    if key[integer.len()..].ends_with('0') {
        return Err(format!("invalid order key: {key}"));
    }
    Ok(())
}

fn increment_integer(x: &str) -> Option<String> {
    let (head, digits) = (x.as_bytes()[0], &x.as_bytes()[1..]);
    let mut digits = digits.to_vec();
    let mut carry = true;
    for d in digits.iter_mut().rev() {
        let next = digit(*d) + 1;
        if next == DIGITS.len() {
            *d = b'0';
        } else {
            *d = DIGITS[next];
            carry = false;
            break;
        }
    }
    if carry {
        if head == b'Z' {
            return Some("a0".into());
        }
        if head == b'z' {
            return None;
        }
        let head = head + 1;
        if head > b'a' {
            digits.push(b'0');
        } else {
            digits.pop();
        }
        digits.insert(0, head);
        return String::from_utf8(digits).ok();
    }
    digits.insert(0, head);
    String::from_utf8(digits).ok()
}

fn decrement_integer(x: &str) -> Option<String> {
    let (head, digits) = (x.as_bytes()[0], &x.as_bytes()[1..]);
    let mut digits = digits.to_vec();
    let mut borrow = true;
    for d in digits.iter_mut().rev() {
        let index = digit(*d);
        if index == 0 {
            *d = DIGITS[DIGITS.len() - 1];
        } else {
            *d = DIGITS[index - 1];
            borrow = false;
            break;
        }
    }
    if borrow {
        if head == b'a' {
            return Some(format!("Z{}", DIGITS[DIGITS.len() - 1] as char));
        }
        if head == b'A' {
            return None;
        }
        let head = head - 1;
        if head < b'Z' {
            digits.push(DIGITS[DIGITS.len() - 1]);
        } else {
            digits.pop();
        }
        digits.insert(0, head);
        return String::from_utf8(digits).ok();
    }
    digits.insert(0, head);
    String::from_utf8(digits).ok()
}

fn join(integer: &str, fraction: Vec<u8>) -> String {
    let mut out = integer.to_owned();
    out.push_str(std::str::from_utf8(&fraction).expect("order key digits are ASCII"));
    out
}

/// A key strictly between `a` and `b`; `None` means the start or end.
pub fn key_between(a: Option<&str>, b: Option<&str>) -> Result<String, String> {
    if let Some(a) = a {
        validate(a)?;
    }
    if let Some(b) = b {
        validate(b)?;
    }
    match (a, b) {
        (Some(a), Some(b)) if a >= b => Err(format!("order key {a} is not before {b}")),
        (None, None) => Ok("a0".into()),
        (None, Some(b)) => {
            let ib = integer_part(b)?;
            let fb = &b[ib.len()..];
            if ib == SMALLEST_INTEGER {
                return Ok(join(ib, midpoint(b"", Some(fb.as_bytes()))));
            }
            if ib < b {
                return Ok(ib.to_owned());
            }
            decrement_integer(ib).ok_or_else(|| "cannot decrement order key".into())
        }
        (Some(a), None) => {
            let ia = integer_part(a)?;
            let fa = &a[ia.len()..];
            Ok(increment_integer(ia).unwrap_or_else(|| join(ia, midpoint(fa.as_bytes(), None))))
        }
        (Some(a), Some(b)) => {
            let (ia, ib) = (integer_part(a)?, integer_part(b)?);
            let (fa, fb) = (&a[ia.len()..], &b[ib.len()..]);
            if ia == ib {
                return Ok(join(ia, midpoint(fa.as_bytes(), Some(fb.as_bytes()))));
            }
            let i = increment_integer(ia).ok_or("cannot increment order key")?;
            if i.as_str() < b {
                return Ok(i);
            }
            Ok(join(ia, midpoint(fa.as_bytes(), None)))
        }
    }
}

/// `n` ascending keys strictly between `a` and `b`.
pub fn keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Result<Vec<String>, String> {
    match n {
        0 => Ok(Vec::new()),
        1 => Ok(vec![key_between(a, b)?]),
        _ if b.is_none() => {
            let mut keys = vec![key_between(a, b)?];
            for _ in 1..n {
                let next = key_between(keys.last().map(String::as_str), b)?;
                keys.push(next);
            }
            Ok(keys)
        }
        _ if a.is_none() => {
            let mut keys = vec![key_between(a, b)?];
            for _ in 1..n {
                let next = key_between(a, keys.last().map(String::as_str))?;
                keys.push(next);
            }
            keys.reverse();
            Ok(keys)
        }
        _ => {
            let middle = n / 2;
            let c = key_between(a, b)?;
            let mut keys = keys_between(a, Some(&c), middle)?;
            keys.push(c.clone());
            keys.extend(keys_between(Some(&c), b, n - middle - 1)?);
            Ok(keys)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_implementation() {
        // Expected values from the fractional-indexing package's test suite.
        let cases = [
            (None, None, "a0"),
            (None, Some("a0"), "Zz"),
            (None, Some("Zz"), "Zy"),
            (Some("a0"), None, "a1"),
            (Some("a1"), None, "a2"),
            (Some("a0"), Some("a1"), "a0V"),
            (Some("a1"), Some("a2"), "a1V"),
            (Some("a0V"), Some("a1"), "a0l"),
            (Some("Zz"), Some("a0"), "ZzV"),
            (Some("Zz"), Some("a1"), "a0"),
            (None, Some("Y00"), "Xzzz"),
            (Some("bzz"), None, "c000"),
            (Some("a0"), Some("a0V"), "a0G"),
            (Some("a0"), Some("a0G"), "a08"),
            (Some("b125"), Some("b129"), "b127"),
            (Some("a0"), Some("a1V"), "a1"),
            (Some("Zz"), Some("a01"), "a0"),
            (None, Some("a0V"), "a0"),
            (None, Some("b999"), "b99"),
        ];
        for (a, b, expected) in cases {
            assert_eq!(
                key_between(a, b).unwrap(),
                expected,
                "between {a:?} and {b:?}"
            );
        }
        assert!(key_between(Some("a0"), Some("a0")).is_err());
        assert!(key_between(Some("a00"), None).is_err());
        assert!(key_between(Some(SMALLEST_INTEGER), None).is_err());
    }

    #[test]
    fn many_keys_are_ordered() {
        for (a, b) in [
            (None, None),
            (Some("a0"), None),
            (None, Some("a0")),
            (Some("a0"), Some("a1")),
        ] {
            let keys = keys_between(a, b, 50).unwrap();
            assert_eq!(keys.len(), 50);
            assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(a.is_none_or(|a| a < keys[0].as_str()));
            assert!(b.is_none_or(|b| keys[49].as_str() < b));
        }
    }
}
