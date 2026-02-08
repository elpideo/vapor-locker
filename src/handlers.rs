use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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
    value: Option<ValueEnc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiCsrfResponse {
    csrf: String,
    field: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiGetRequest {
    hashes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ApiSetRequest {
    key_hash: Option<String>,
    value: Option<ValueEnc>,
    ephemeral: Option<bool>,
    csrf: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ValueEnc {
    v: u8,
    iv: String,
    ct: String,
}

#[derive(Debug, Serialize)]
struct ApiSaltsResponse {
    salts: Vec<String>,
}

fn json<T: Serialize>(status: StatusCode, payload: T) -> Response {
    (status, Json(payload)).into_response()
}

pub async fn api_salts(State(state): State<AppState>) -> Response {
    let salts = match state.db.list_valid_salts_with_rotation().await {
        Ok(v) => v,
        Err(e) => {
            return json(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiOk {
                    ok: false,
                    error: Some(format!("Internal error: {e}")),
                },
            )
        }
    };

    let salts_b64: Vec<String> = salts.into_iter().map(|s| URL_SAFE_NO_PAD.encode(s)).collect();
    json(StatusCode::OK, ApiSaltsResponse { salts: salts_b64 })
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

    let key_hash = req.key_hash.unwrap_or_default();
    if key_hash.is_empty() {
        return json(
            StatusCode::BAD_REQUEST,
            ApiOk {
                ok: false,
                error: Some("Validation error: key_hash required".to_string()),
            },
        );
    }

    let Some(value_enc) = req.value else {
        return json(
            StatusCode::BAD_REQUEST,
            ApiOk {
                ok: false,
                error: Some("Validation error: value required".to_string()),
            },
        );
    };
    let ephemeral = req.ephemeral.unwrap_or(false);

    let value_json = match serde_json::to_string(&value_enc) {
        Ok(s) => s,
        Err(e) => {
            return json(
                StatusCode::BAD_REQUEST,
                ApiOk {
                    ok: false,
                    error: Some(format!("Validation error: invalid value ({e})")),
                },
            )
        }
    };

    let validated = match (models::SetInput {
        key: key_hash.clone(),
        value: value_json.clone(),
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
        key_hash_len = validated.key.chars().count(),
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
    Json(req): Json<ApiGetRequest>,
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

    let hashes = req.hashes.unwrap_or_default();
    if hashes.is_empty() {
        return json(
            StatusCode::BAD_REQUEST,
            ApiGetResponse {
                found: false,
                value: None,
                error: Some("Validation error: hashes required".to_string()),
            },
        );
    }
    if hashes.len() > 256 {
        return json(
            StatusCode::BAD_REQUEST,
            ApiGetResponse {
                found: false,
                value: None,
                error: Some("Validation error: too many hashes".to_string()),
            },
        );
    }

    let mut validated_hashes = Vec::with_capacity(hashes.len());
    for h in hashes {
        if h.is_empty() {
            return json(
                StatusCode::BAD_REQUEST,
                ApiGetResponse {
                    found: false,
                    value: None,
                    error: Some("Validation error: empty hash".to_string()),
                },
            );
        }
        let v = match (models::GetInput { key: h }).validate() {
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
        validated_hashes.push(v.key);
    }

    let value = match state
        .db
        .get_value_by_hashes_maybe_delete_ephemeral(validated_hashes)
        .await
    {
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
        Some(v) => {
            let parsed: ValueEnc = match serde_json::from_str(&v) {
                Ok(p) => p,
                Err(e) => {
                    return json(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiGetResponse {
                            found: false,
                            value: None,
                            error: Some(format!("Internal error: corrupted payload ({e})")),
                        },
                    )
                }
            };
            json(
                StatusCode::OK,
                ApiGetResponse {
                    found: true,
                    value: Some(parsed),
                    error: None,
                },
            )
        }
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

