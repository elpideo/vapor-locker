use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{models, AppState};

#[derive(Debug, Serialize)]
struct ApiOk {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiGetResponse {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiCsrfResponse {
    csrf: String,
    field: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiGetQuery {
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiSetRequest {
    key: Option<String>,
    value: Option<String>,
    ephemeral: Option<bool>,
    csrf: Option<String>,
}

fn json<T: Serialize>(status: StatusCode, payload: T) -> Response {
    (status, Json(payload)).into_response()
}

pub async fn api_csrf(
    State(state): State<AppState>,
    cookies: tower_cookies::Cookies,
) -> Response {
    let token = match state.csrf.issue_token(&cookies) {
        Ok(t) => t,
        Err(e) => return json(StatusCode::INTERNAL_SERVER_ERROR, ApiOk { ok: false, error: Some(format!("Internal error: {e}")) }),
    };

    json(
        StatusCode::OK,
        ApiCsrfResponse {
            csrf: token,
            field: state.csrf.field_name.clone(),
        },
    )
}

pub async fn api_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    cookies: tower_cookies::Cookies,
    Json(req): Json<ApiSetRequest>,
) -> Response {
    // CSRF check (required for writes)
    if let Err(_r) = state.csrf.verify(&cookies, req.csrf.as_deref()) {
        return json(
            StatusCode::FORBIDDEN,
            ApiOk {
                ok: false,
                error: Some("Forbidden".to_string()),
            },
        );
    }

    let ip = client_ip(&headers, addr, state.trust_proxy);
    if state.ip_cache.seen_recently(ip) {
        return json(StatusCode::OK, ApiOk { ok: true, error: None });
    }

    let key = req.key.unwrap_or_default();
    let value = req.value.unwrap_or_default();
    let ephemeral = req.ephemeral.unwrap_or(false);

    let validated = match (models::SetInput {
        key: key.clone(),
        value: value.clone(),
        ephemeral,
    })
    .validate() {
        Ok(v) => v,
        Err(e) => {
            return json(
                StatusCode::BAD_REQUEST,
                ApiOk {
                    ok: false,
                    error: Some(format!("Validation error: {e}")),
                },
            )
        }
    };

    // Log (no content)
    info!(
        event = "set",
        ip = %ip,
        key_len = validated.key.chars().count(),
        value_len = validated.value.chars().count(),
        ephemeral = validated.ephemeral
    );

    if let Err(e) = state
        .db
        .insert(&validated.key, &validated.value, validated.ephemeral)
        .await
    {
        return json(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiOk {
                ok: false,
                error: Some(format!("Internal error: {e}")),
            },
        );
    }

    json(StatusCode::OK, ApiOk { ok: true, error: None })
}

pub async fn api_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(q): Query<ApiGetQuery>,
) -> Response {
    let ip = client_ip(&headers, addr, state.trust_proxy);

    // Log (ip only as requested)
    info!(event = "get", ip = %ip);

    if state.ip_cache.seen_recently(ip) {
        return json(
            StatusCode::OK,
            ApiGetResponse {
                found: false,
                value: None,
                error: None,
            },
        );
    }

    let key = q.key.unwrap_or_default();
    let validated = match (models::GetInput { key }).validate() {
        Ok(v) => v,
        Err(e) => {
            return json(
                StatusCode::BAD_REQUEST,
                ApiGetResponse {
                    found: false,
                    value: None,
                    error: Some(format!("Validation error: {e}")),
                },
            )
        }
    };

    let value = match state.db.get_value_maybe_delete_ephemeral(&validated.key).await {
        Ok(v) => v,
        Err(e) => {
            return json(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiGetResponse {
                    found: false,
                    value: None,
                    error: Some(format!("Internal error: {e}")),
                },
            )
        }
    };

    match value {
        Some(v) => json(
            StatusCode::OK,
            ApiGetResponse {
                found: true,
                value: Some(v),
                error: None,
            },
        ),
        None => json(
            StatusCode::OK,
            ApiGetResponse {
                found: false,
                value: None,
                error: None,
            },
        ),
    }
}

fn client_ip(headers: &HeaderMap, addr: SocketAddr, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = xff.split(',').next().map(|s| s.trim()) {
                if let Ok(ip) = first.parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }
    addr.ip()
}

