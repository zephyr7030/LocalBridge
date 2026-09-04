use std::fmt::Write as _;

pub(crate) fn random_hex(bytes: usize) -> Result<String, getrandom::Error> {
    let mut random = vec![0_u8; bytes];
    getrandom::fill(&mut random)?;
    let mut encoded = String::with_capacity(bytes.saturating_mul(2));
    for byte in random {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

pub(crate) fn random_prefixed_id(prefix: &str) -> String {
    let random = random_hex(24).expect("operating-system CSPRNG is required");
    format!("{prefix}{random}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_values_have_fixed_shape_and_do_not_repeat() {
        let first = random_prefixed_id("lb-");
        let second = random_prefixed_id("lb-");
        assert_eq!(first.len(), 51);
        assert_eq!(second.len(), 51);
        assert_ne!(first, second);
        assert!(first[3..].bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
