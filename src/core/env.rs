/// Parse an env:VAR_NAME reference and resolve the value.
/// - "env:FOO" → reads $FOO, falls back to $CLAUDE_API_KEY, then ""
/// - "literal-key" → returns as-is
/// - "" → reads $CLAUDE_API_KEY, then ""
pub fn resolve_api_key(raw: &str) -> String {
    if let Some(var_name) = raw.strip_prefix("env:") {
        std::env::var(var_name)
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .unwrap_or_default()
    } else if raw.is_empty() {
        std::env::var("CLAUDE_API_KEY").unwrap_or_default()
    } else {
        raw.to_string()
    }
}

/// Extract variable name from env:XXX reference, returns None if not a reference
#[allow(dead_code)]
pub fn parse_env_ref(raw: &str) -> Option<String> {
    raw.strip_prefix("env:").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_literal_key() {
        let result = resolve_api_key("sk-abc123");
        assert_eq!(result, "sk-abc123");
    }

    #[test]
    fn test_resolve_env_ref() {
        std::env::set_var("TEST_KEY", "test-value");
        let result = resolve_api_key("env:TEST_KEY");
        assert_eq!(result, "test-value");
    }

    #[test]
    fn test_resolve_env_ref_fallback() {
        std::env::set_var("CLAUDE_API_KEY", "fallback-key");
        let result = resolve_api_key("env:NONEXISTENT_VAR");
        assert_eq!(result, "fallback-key");
    }

    #[test]
    fn test_resolve_empty_fallback() {
        std::env::set_var("CLAUDE_API_KEY", "default-key");
        let result = resolve_api_key("");
        assert_eq!(result, "default-key");
    }

    #[test]
    fn test_parse_env_ref_some() {
        assert_eq!(parse_env_ref("env:FOO"), Some("FOO".to_string()));
    }

    #[test]
    fn test_parse_env_ref_none() {
        assert_eq!(parse_env_ref("literal"), None);
    }
}
