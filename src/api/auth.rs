use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;

pub(super) fn is_authorized(headers: &HeaderMap, secrets: &[String]) -> bool {
    let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some((scheme, token)) = value.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    secrets
        .iter()
        .any(|secret| constant_time_eq(token.as_bytes(), secret.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_auth_uses_constant_time_comparison() {
        let secrets = vec!["change-me".to_string()];
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer change-me".parse().unwrap());
        assert!(is_authorized(&headers, &secrets));

        headers.insert(AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert!(!is_authorized(&headers, &secrets));

        headers.insert(AUTHORIZATION, "Basic change-me".parse().unwrap());
        assert!(!is_authorized(&headers, &secrets));

        headers.remove(AUTHORIZATION);
        assert!(!is_authorized(&headers, &secrets));
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
