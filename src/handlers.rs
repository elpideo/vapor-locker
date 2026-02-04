use std::{
    net::{IpAddr, SocketAddr},
};

use axum::{
    extract::{ConnectInfo, Form, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use tracing::info;

use crate::{csrf, models, AppState};

#[derive(Debug, Deserialize)]
pub struct SetForm {
    key: Option<String>,
    value: Option<String>,
    ephemeral: Option<String>,
    csrf: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetForm {
    key: Option<String>,
    csrf: Option<String>,
}

pub async fn get_set_form(
    State(state): State<AppState>,
    cookies: tower_cookies::Cookies,
) -> Result<Html<String>, Response> {
    let token = state
        .csrf
        .issue_token(&cookies)
        .map_err(internal_error)?;

    Ok(Html(render_set_form(
        &state.csrf.field_name,
        &token,
        None,
    )))
}

pub async fn post_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    cookies: tower_cookies::Cookies,
    Form(form): Form<SetForm>,
) -> Result<Html<String>, Response> {
    // CSRF check
    state
        .csrf
        .verify(&cookies, form.csrf.as_deref())
        .map_err(|r| r)?;

    let ip = client_ip(&headers, addr, state.trust_proxy);
    if state.ip_cache.seen_recently(ip) {
        return Ok(Html(render_set_ok()));
    }

    let key = form.key.unwrap_or_default();
    let value = form.value.unwrap_or_default();
    let ephemeral = form.ephemeral.is_some();

    let validated = models::SetInput {
        key: key.clone(),
        value: value.clone(),
        ephemeral,
    }
    .validate()
    .map_err(|e| {
        let msg = format!("Validation error: {e}");
        Html(render_set_form(
            &state.csrf.field_name,
            &state.csrf.issue_token(&cookies).unwrap_or_default(),
            Some(&msg),
        ))
        .into_response()
    })?;

    // Log (no content)
    info!(
        event = "set",
        ip = %ip,
        key_len = validated.key.chars().count(),
        value_len = validated.value.chars().count(),
        ephemeral = validated.ephemeral
    );

    state
        .db
        .insert(&validated.key, &validated.value, validated.ephemeral)
        .await
        .map_err(internal_error)?;

    Ok(Html(render_set_ok()))
}

pub async fn get_get_form(
    State(state): State<AppState>,
    cookies: tower_cookies::Cookies,
) -> Result<Html<String>, Response> {
    let token = state
        .csrf
        .issue_token(&cookies)
        .map_err(internal_error)?;
    Ok(Html(render_get_form(
        &state.csrf.field_name,
        &token,
        None,
    )))
}

pub async fn post_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    cookies: tower_cookies::Cookies,
    Form(form): Form<GetForm>,
) -> Result<Html<String>, Response> {
    state
        .csrf
        .verify(&cookies, form.csrf.as_deref())
        .map_err(|r| r)?;

    let ip = client_ip(&headers, addr, state.trust_proxy);

    // Log (ip only as requested)
    info!(event = "get", ip = %ip);

    if state.ip_cache.seen_recently(ip) {
        return Ok(Html(render_get_result(
            &state.csrf.field_name,
            &state.csrf.issue_token(&cookies).unwrap_or_default(),
            None,
        )));
    }

    let key = form.key.unwrap_or_default();
    let validated = models::GetInput { key }
        .validate()
        .map_err(|e| {
            let msg = format!("Validation error: {e}");
            Html(render_get_form(
                &state.csrf.field_name,
                &state.csrf.issue_token(&cookies).unwrap_or_default(),
                Some(&msg),
            ))
            .into_response()
        })?;

    let value = state
        .db
        .get_value_maybe_delete_ephemeral(&validated.key)
        .await
        .map_err(internal_error)?;

    let token = state
        .csrf
        .issue_token(&cookies)
        .map_err(internal_error)?;
    Ok(Html(render_get_result(
        &state.csrf.field_name,
        &token,
        value.as_deref(),
    )))
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

fn internal_error<E: std::fmt::Display>(e: E) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("Internal error: {e}")).into_response()
}

fn base_page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width,initial-scale=1"/>
  <title>{title}</title>
  <style>
    body{{font-family:system-ui,-apple-system,Segoe UI,Roboto,Ubuntu,Cantarell,Noto Sans,sans-serif;max-width:860px;margin:40px auto;padding:0 16px;}}
    header{{display:flex;gap:16px;align-items:center;justify-content:space-between;margin-bottom:24px;}}
    nav a{{margin-right:12px;}}
    label{{display:block;margin:12px 0 6px;font-weight:600;}}
    input[type=text], textarea{{width:100%;padding:10px;border:1px solid #ccc;border-radius:8px;}}
    textarea{{min-height:220px;}}
    .row{{display:flex;gap:12px;align-items:center;}}
    .btn{{margin-top:16px;padding:10px 16px;border-radius:10px;border:1px solid #111;background:#111;color:#fff;cursor:pointer;}}
    .msg{{margin:12px 0;padding:10px 12px;border-radius:10px;background:#f6f6f6;border:1px solid #e4e4e4;}}
    .value{{white-space:pre-wrap;border:1px solid #ddd;border-radius:10px;padding:12px;background:#fafafa;}}
    code{{background:#f1f1f1;padding:2px 6px;border-radius:6px;}}
  </style>
</head>
<body>
  <header>
    <div><strong>Vapor</strong> — KV éphémère</div>
    <nav>
      <a href="/">Entrée</a>
      <a href="/get">Récupération</a>
    </nav>
  </header>
  {body}
</body>
</html>
"#,
        title = html_escape(title),
        body = body
    )
}

fn render_set_form(csrf_field: &str, token: &str, message: Option<&str>) -> String {
    let msg = message
        .map(|m| format!(r#"<div class="msg">{}</div>"#, html_escape(m)))
        .unwrap_or_default();
    base_page(
        "Entrée",
        &format!(
            r#"
{msg}
<form method="post" action="/set">
  {csrf}
  <label for="key">key (max 255)</label>
  <input id="key" name="key" type="text" maxlength="255" />

  <label for="value">valeur (max 100000)</label>
  <textarea id="value" name="value" maxlength="100000"></textarea>

  <div class="row">
    <input id="ephemeral" name="ephemeral" type="checkbox" />
    <label for="ephemeral" style="margin:0;font-weight:500;">éphémère (supprimé à la lecture)</label>
  </div>

  <button class="btn" type="submit">Valider</button>
</form>
"#,
            msg = msg,
            csrf = csrf::hidden_input(csrf_field, token)
        ),
    )
}

fn render_set_ok() -> String {
    base_page(
        "OK",
        r#"<div class="msg">OK</div><p>Tu peux récupérer via <code>/get</code>.</p>"#,
    )
}

fn render_get_form(csrf_field: &str, token: &str, message: Option<&str>) -> String {
    let msg = message
        .map(|m| format!(r#"<div class="msg">{}</div>"#, html_escape(m)))
        .unwrap_or_default();
    base_page(
        "Récupération",
        &format!(
            r#"
{msg}
<form method="post" action="/get">
  {csrf}
  <label for="key">key</label>
  <input id="key" name="key" type="text" maxlength="255" />
  <button class="btn" type="submit">Valider</button>
</form>
"#,
            msg = msg,
            csrf = csrf::hidden_input(csrf_field, token)
        ),
    )
}

fn render_get_result(csrf_field: &str, token: &str, value: Option<&str>) -> String {
    let content = match value {
        None => r#"<div class="msg">non trouvé</div>"#.to_string(),
        Some(v) => format!(
            r#"<div class="msg">trouvé</div><div class="value">{}</div>"#,
            html_escape(v)
        ),
    };

    base_page(
        "Résultat",
        &format!(
            r#"
{content}
<hr style="margin:24px 0;border:none;border-top:1px solid #eee;"/>
<form method="post" action="/get">
  {csrf}
  <label for="key">key</label>
  <input id="key" name="key" type="text" maxlength="255" />
  <button class="btn" type="submit">Rechercher</button>
</form>
"#,
            content = content,
            csrf = csrf::hidden_input(csrf_field, token)
        ),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

