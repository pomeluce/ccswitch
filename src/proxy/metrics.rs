use crate::core::config::ConfigManager;
use reqwest::Response;

/// Extract token usage from upstream response headers and record them in the database.
/// Anthropic-compatible APIs return x-usage-prompt-tokens / x-usage-completion-tokens
/// in response headers. These are best-effort; streaming responses may not include them.
///
/// NOTE: This is deliberately a synchronous function — it is called while holding
/// a `MutexGuard<ConfigManager>`, and never holds that guard across an await point.
pub fn record_metrics(mgr: &ConfigManager, resp: &Response) -> anyhow::Result<()> {
    let prompt_tokens: i64 = resp
        .headers()
        .get("x-usage-prompt-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let completion_tokens: i64 = resp
        .headers()
        .get("x-usage-completion-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if prompt_tokens > 0 || completion_tokens > 0 {
        let provider = mgr.get_setting("active_provider").unwrap_or_default();
        let profile = mgr.get_setting("active_profile").unwrap_or_default();
        mgr.db()
            .insert_usage_log("claude", &provider, &profile, None, prompt_tokens, completion_tokens, 0, 0, "proxy")?;
    }
    Ok(())
}
