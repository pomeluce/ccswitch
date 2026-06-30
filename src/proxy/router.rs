use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use reqwest::Client;

use crate::core::config::ConfigManager;
use crate::core::env::resolve_api_key;

use super::metrics::record_metrics;

/// Shared proxy state, held behind an Arc<Mutex<>> because `rusqlite::Connection`
/// uses internal `RefCell` and is therefore not `Sync`.
pub struct ProxyState {
    pub mgr: Arc<Mutex<ConfigManager>>,
    pub client: Client,
}

/// Handle all incoming Anthropic-compatible API requests.
///
/// Reads the active provider/profile from SQLite, replaces the Authorization
/// header, forwards the body upstream, and streams the response back.
pub async fn proxy_handler(
    State(state): State<Arc<ProxyState>>,
    req: Request<Body>,
) -> Response {
    // ── Extract request data we need BEFORE consuming the body ──────
    let method = req.method().clone();
    let original_headers = req.headers().clone();
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    // Resolve the active upstream target and auth token
    let (target_url, auth_token) = {
        let mgr = match state.mgr.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("Mutex poisoned: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".to_string())
                    .into_response();
            }
        };
        match get_active_upstream(&mgr) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Failed to resolve upstream: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
        }
    };

    // Build upstream URL preserving path + query
    let upstream_url = format!("{}{}", target_url.trim_end_matches('/'), path);

    // Read entire request body (API requests are typically small)
    let body_bytes = match axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Build upstream request with replaced auth header
    let headers = replace_auth_header(&original_headers, &auth_token);

    let upstream_req = state
        .client
        .request(method, &upstream_url)
        .headers(headers)
        .body(reqwest::Body::from(body_bytes))
        .build();

    let upstream_req = match upstream_req {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to build upstream request: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build upstream request")
                .into_response();
        }
    };

    // Execute the upstream request
    match state.client.execute(upstream_req).await {
        Ok(resp) => {
            // Record metrics (best-effort)
            match state.mgr.lock() {
                Ok(mgr) => {
                    if let Err(e) = record_metrics(&mgr, &resp) {
                        tracing::warn!("Failed to record metrics: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Mutex poisoned during metrics recording: {}", e);
                }
            }

            // Stream the upstream response back to the caller
            let status = resp.status();
            let response_headers = resp.headers().clone();
            let body = resp.bytes_stream();

            let mut response = Response::new(Body::from_stream(body));
            *response.status_mut() = status;
            *response.headers_mut() = response_headers;
            response
        }
        Err(e) => {
            tracing::error!("Upstream request failed: {}", e);
            (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", e)).into_response()
        }
    }
}

/// Look up the active provider and profile from the database and return
/// the upstream base URL and resolved API token.
fn get_active_upstream(mgr: &ConfigManager) -> anyhow::Result<(String, String)> {
    let provider_id = mgr
        .db()
        .get_setting("active_provider")
        .ok_or_else(|| anyhow::anyhow!("No active provider set"))?;
    let profile_id = mgr
        .db()
        .get_setting("active_profile")
        .ok_or_else(|| anyhow::anyhow!("No active profile set"))?;

    let (provider, _profile) = mgr
        .find_profile(&provider_id, &profile_id)?
        .ok_or_else(|| anyhow::anyhow!("Profile {}/{} not found", provider_id, profile_id))?;

    let token = resolve_api_key(&provider.api_key);
    Ok((provider.api_url, token))
}

/// Clone the original headers and replace the Authorization header with the
/// resolved API token.
fn replace_auth_header(original: &HeaderMap, new_token: &str) -> HeaderMap {
    let mut headers = original.clone();
    if let Ok(hv) = HeaderValue::from_str(&format!("Bearer {}", new_token)) {
        headers.insert("Authorization", hv);
    }
    headers
}
