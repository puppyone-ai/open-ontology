#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allowed {
    Models,
    Chat,
}

pub fn allow(method: &str, path: &str) -> Option<Allowed> {
    match (method, path) {
        ("GET", "/v1/models") => Some(Allowed::Models),
        ("POST", "/v1/chat/completions") => Some(Allowed::Chat),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::allow;
    use super::Allowed;

    #[test]
    fn allowlist_is_exact() {
        assert_eq!(allow("GET", "/v1/models"), Some(Allowed::Models));
        assert_eq!(allow("POST", "/v1/chat/completions"), Some(Allowed::Chat));
        assert_eq!(allow("POST", "/v1/models"), None);
        assert_eq!(allow("GET", "/v1/chat/completions"), None);
        assert_eq!(allow("CONNECT", "/v1/models"), None);
        assert_eq!(allow("GET", "/v1/models?x=1"), None);
        assert_eq!(allow("GET", "/v1/models/"), None);
        assert_eq!(allow("GET", "http://127.0.0.1/v1/models"), None);
    }
}
