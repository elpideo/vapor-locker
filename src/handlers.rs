use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tracing::info;

use crate::{models, AppState};

/// Réponse JSON générique `{ ok, error? }`.
#[derive(Debug, Serialize)]
struct ApiOk {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Réponse JSON de `POST /api/get`.
#[derive(Debug, Serialize)]
struct ApiGetResponse {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<ValueEnc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ephemeral: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Réponse JSON de `GET /api/csrf`.
#[derive(Debug, Serialize)]
struct ApiCsrfResponse {
    csrf: String,
    field: String,
}

/// Requête JSON de `POST /api/get`.
///
/// `hashes` contient une liste de `key_hash` possibles (un par sel valide côté client).
#[derive(Debug, Deserialize)]
pub struct ApiGetRequest {
    hashes: Option<Vec<String>>,
}

/// Requête JSON de `POST /api/set`.
///
/// Le serveur reçoit uniquement :
/// - un `key_hash` (jamais la clé en clair)
/// - une valeur chiffrée (`value`)
/// - un token CSRF (`csrf`) pour autoriser l’écriture
#[derive(Debug, Deserialize)]
pub struct ApiSetRequest {
    key_hash: Option<String>,
    value: Option<ValueEnc>,
    ephemeral: Option<bool>,
    csrf: Option<String>,
}

/// Valeur chiffrée “transport” (le serveur ne déchiffre pas).
///
/// Stockée en DB sous forme JSON, puis renvoyée telle quelle au client.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ValueEnc {
    v: u8,
    iv: String,
    ct: String,
}

/// Réponse JSON de `GET /api/salts`.
#[derive(Debug, Serialize)]
struct ApiSaltsResponse {
    salts: Vec<String>,
}

/// Réponse JSON de `GET /api/version`.
#[derive(Debug, Serialize)]
struct ApiVersionResponse {
    version: String,
}

/// Helper pour retourner une réponse JSON avec un status HTTP. 
fn json<T: Serialize>(status: StatusCode, payload: T) -> Response {
    (status, Json(payload)).into_response()
}

/// Réponse 429 avec en-tête Retry-After (secondes).
fn json_429_retry<T: Serialize>(payload: T, retry_after_secs: u64) -> Response {
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        headers.insert(header::RETRY_AFTER, v);
    }
    (StatusCode::TOO_MANY_REQUESTS, headers, Json(payload)).into_response()
}

/// `GET /api/salts`
///
/// Retourne la liste des sels valides (base64 URL-safe, sans padding) et assure la rotation.
/// La réponse est explicitement marquée comme **non-cachable** côté client (`Cache-Control: no-store`).
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

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));

    (StatusCode::OK, headers, Json(ApiSaltsResponse { salts: salts_b64 })).into_response()
}

/// `GET /api/version`
///
/// Retourne la version de l'application, issue de `Cargo.toml`.
pub async fn api_version() -> Response {
    json(
        StatusCode::OK,
        ApiVersionResponse {
            version: crate::version::APP_VERSION.to_string(),
        },
    )
}

/// `GET /api/csrf`
///
/// Émet un token CSRF (et pose le cookie associé).
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

/// `POST /api/set`
///
/// Valide CSRF + payload, applique l’anti-rejeu IP (fenêtre courte), puis insère en DB.
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
    if let Err(retry_after_secs) = state.abuse_limiter.check_or_update(ip) {
        return json_429_retry(
            ApiOk {
                ok: false,
                error: Some("too many requests".to_string()),
            },
            retry_after_secs,
        );
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

/// `POST /api/get`
///
/// Valide la liste de hashes, applique l’anti-rejeu IP, puis retourne la dernière valeur non expirée.
pub async fn api_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<ApiGetRequest>,
) -> Response {
    let ip = client_ip(&headers, addr, state.trust_proxy);

    // Log (ip only as requested)
    info!(event = "get", ip = %ip);

    if let Err(retry_after_secs) = state.abuse_limiter.check_or_update(ip) {
        return json_429_retry(
            ApiGetResponse {
                found: false,
                value: None,
                ttl_secs: None,
                ephemeral: None,
                error: Some("too many requests".to_string()),
            },
            retry_after_secs,
        );
    }

    let hashes = req.hashes.unwrap_or_default();
    if hashes.is_empty() {
        return json(
            StatusCode::BAD_REQUEST,
            ApiGetResponse {
                found: false,
                value: None,
                ttl_secs: None,
                ephemeral: None,
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
                ttl_secs: None,
                ephemeral: None,
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
                    ttl_secs: None,
                    ephemeral: None,
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
                        ttl_secs: None,
                        ephemeral: None,
                        error: Some(format!("Validation error: {e}")),
                    },
                )
            }
        };
        validated_hashes.push(v.key);
    }

    let entry = match state
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
                    ttl_secs: None,
                    ephemeral: None,
                    error: Some(format!("Internal error: {e}")),
                },
            )
        }
    };

    match entry {
        Some(e) => {
            let parsed: ValueEnc = match serde_json::from_str(&e.value) {
                Ok(p) => p,
                Err(err) => {
                    return json(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiGetResponse {
                            found: false,
                            value: None,
                            ttl_secs: None,
                            ephemeral: None,
                            error: Some(format!("Internal error: corrupted payload ({err})")),
                        },
                    )
                }
            };

            let now = OffsetDateTime::now_utc();
            let ttl = Duration::hours(24) - (now - e.created_at);
            let ttl_secs = ttl.whole_seconds().max(0) as u64;
            json(
                StatusCode::OK,
                ApiGetResponse {
                    found: true,
                    value: Some(parsed),
                    ttl_secs: Some(ttl_secs),
                    ephemeral: Some(e.ephemeral),
                    error: None,
                },
            )
        }
        None => json(
            StatusCode::OK,
            ApiGetResponse {
                found: false,
                value: None,
                ttl_secs: None,
                ephemeral: None,
                error: None,
            },
        ),
    }
}

/// Détermine l’IP client.
///
/// Si `trust_proxy=true`, tente `x-forwarded-for` (prend la première IP); sinon utilise l’IP socket.
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

