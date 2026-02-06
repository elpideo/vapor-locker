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

    Ok(Html(render_app_page(&state.csrf.field_name, &token, None)))
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
        let token = state
            .csrf
            .issue_token(&cookies)
            .map_err(internal_error)?;
        return Ok(Html(render_app_page(
            &state.csrf.field_name,
            &token,
            Some(ResultBlock::Message("OK".to_string())),
        )));
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
        Html(render_app_page(
            &state.csrf.field_name,
            &state.csrf.issue_token(&cookies).unwrap_or_default(),
            Some(ResultBlock::Message(msg)),
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

    let token = state
        .csrf
        .issue_token(&cookies)
        .map_err(internal_error)?;
    Ok(Html(render_app_page(
        &state.csrf.field_name,
        &token,
        Some(ResultBlock::Message("OK".to_string())),
    )))
}

pub async fn get_get_form(
    State(state): State<AppState>,
    cookies: tower_cookies::Cookies,
) -> Result<Html<String>, Response> {
    let token = state
        .csrf
        .issue_token(&cookies)
        .map_err(internal_error)?;
    Ok(Html(render_app_page(&state.csrf.field_name, &token, None)))
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
        return Ok(Html(render_app_page(
            &state.csrf.field_name,
            &state.csrf.issue_token(&cookies).unwrap_or_default(),
            Some(ResultBlock::Message("Not found".to_string())),
        )));
    }

    let key = form.key.unwrap_or_default();
    let validated = models::GetInput { key }
        .validate()
        .map_err(|e| {
            let msg = format!("Validation error: {e}");
            Html(render_app_page(
                &state.csrf.field_name,
                &state.csrf.issue_token(&cookies).unwrap_or_default(),
                Some(ResultBlock::Message(msg)),
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
    let result = match value {
        Some(v) => Some(ResultBlock::Value(v)),
        None => Some(ResultBlock::Message("Non trouvé".to_string())),
    };
    Ok(Html(render_app_page(&state.csrf.field_name, &token, result)))
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
  <meta name="color-scheme" content="dark"/>
  <style>
    :root{{
      --bg0:#07080a;
      --bg1:#0b0c0f;
      --card:#13151a;
      --card2:#101217;
      --border:#222631;
      --muted:#9aa3b2;
      --text:#e8ecf3;
      --field:#1a1d24;
      --fieldBorder:#2a2f3c;
      --accent:#68ffb6;
      --accentText:#07110b;
      --shadow:0 18px 50px rgba(0,0,0,.55);
      --radius:18px;
    }}
    *{{box-sizing:border-box;}}
    html,body{{height:100%;}}
    body{{
      margin:0;
      font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, sans-serif;
      color:var(--text);
      background:
        radial-gradient(900px 560px at 20% -10%, rgba(104,255,182,.10), transparent 60%),
        radial-gradient(800px 520px at 100% 0%, rgba(104,255,182,.06), transparent 58%),
        linear-gradient(180deg, var(--bg0), var(--bg1));
    }}
    .wrap{{max-width:720px;margin:0 auto;padding:56px 18px 56px;}}
    .brand{{
      font-weight:800;
      letter-spacing:.06em;
      font-size:46px;
      line-height:1.05;
    }}
    .brandDot{{color:var(--accent);}}

    .stack{{display:flex;flex-direction:column;gap:18px;}}
    .card{{
      background: linear-gradient(180deg, rgba(19,21,26,1), rgba(15,17,22,1));
      border:1px solid var(--border);
      border-radius:var(--radius);
      box-shadow: var(--shadow);
      padding:18px;
    }}
    .cardTitle{{
      font-weight:800;
      letter-spacing:.18em;
      color:var(--accent);
      font-size:14px;
      margin-bottom:14px;
    }}
    .fieldRow{{display:flex;gap:12px;align-items:center;}}
    .fieldCol{{display:flex;flex-direction:column;gap:12px;}}
    input[type=text], textarea{{
      width:100%;
      background:var(--field);
      color:var(--text);
      border:1px solid var(--fieldBorder);
      border-radius:14px;
      padding:14px 14px;
      outline:none;
    }}
    input[type=text]{{height:52px;}}
    textarea{{min-height:140px;resize:vertical;}}
    input[type=text]::placeholder, textarea::placeholder{{color:rgba(154,163,178,.65);}}
    input[type=text]:focus, textarea:focus{{border-color:rgba(104,255,182,.55);box-shadow:0 0 0 4px rgba(104,255,182,.10);}}

    .actionBtn{{
      flex:0 0 auto;
      width:52px;height:52px;
      border-radius:14px;
      border:0;
      background:var(--accent);
      color:var(--accentText);
      display:inline-flex;
      align-items:center;
      justify-content:center;
      cursor:pointer;
      box-shadow: 0 14px 30px rgba(104,255,182,.18);
      transition: transform .08s ease, filter .08s ease;
    }}
    .actionBtn:active{{transform: translateY(1px);filter:brightness(.95);}}
    .icon{{width:22px;height:22px;display:block;}}

    .resultSurface{{
      position:relative;
      background:var(--card2);
      border:1px solid var(--border);
      border-radius:14px;
      padding:14px 44px 14px 14px;
      min-height:56px;
    }}
    .resultText{{
      margin:0;
      font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", \"Courier New\", monospace;
      font-size:14px;
      line-height:1.45;
      white-space:pre-wrap;
      word-break:break-word;
      color:rgba(232,236,243,.86);
    }}
    .copyBtn{{
      position:absolute;
      top:10px;right:10px;
      width:32px;height:32px;
      border-radius:10px;
      border:1px solid rgba(255,255,255,.08);
      background: rgba(255,255,255,.04);
      color: rgba(232,236,243,.85);
      cursor:pointer;
      display:flex;
      align-items:center;
      justify-content:center;
    }}
    .copyBtn:active{{transform: translateY(1px);}}
    .copyBtn.copied{{background: rgba(104,255,182,.16);border-color: rgba(104,255,182,.30);color: var(--accent);}}

    .divider{{
      height:1px;
      margin:6px 10px;
      background: linear-gradient(90deg, transparent, rgba(255,255,255,.12), transparent);
      border-radius:999px;
    }}

    .pill{{
      display:inline-flex;
      align-items:center;
      gap:10px;
      padding:10px 14px;
      border-radius:999px;
      background: rgba(255,255,255,.04);
      border:1px solid rgba(255,255,255,.10);
      color: rgba(232,236,243,.9);
      font-weight:650;
    }}
    .muted{{color:var(--muted);font-weight:550;}}
    .rowBetween{{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-top:12px;}}
    .check{{display:inline-flex;align-items:center;gap:10px;color:rgba(232,236,243,.86);}}
    .check input{{width:16px;height:16px;}}
    .srOnly{{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0;}}
  </style>
</head>
<body>
  <main class="wrap">
    {body}
  </main>
  <script>
    (function () {{
      function copyText(text) {{
        if (navigator && navigator.clipboard && navigator.clipboard.writeText) {{
          return navigator.clipboard.writeText(text);
        }}
        var ta = document.createElement('textarea');
        ta.value = text;
        ta.setAttribute('readonly', '');
        ta.style.position = 'absolute';
        ta.style.left = '-9999px';
        document.body.appendChild(ta);
        ta.select();
        try {{ document.execCommand('copy'); }} catch (e) {{}}
        document.body.removeChild(ta);
        return Promise.resolve();
      }}

      var btn = document.querySelector('[data-copy-target]');
      if (!btn) return;
      btn.addEventListener('click', function () {{
        var id = btn.getAttribute('data-copy-target');
        var el = document.getElementById(id);
        if (!el) return;
        var text = (el.innerText || el.textContent || '').trimEnd();
        copyText(text).then(function () {{
          btn.classList.add('copied');
          window.setTimeout(function () {{ btn.classList.remove('copied'); }}, 900);
        }});
      }});
    }})();
  </script>
</body>
</html>
"#,
        title = html_escape(title),
        body = body
    )
}

#[derive(Debug, Clone)]
enum ResultBlock {
    Message(String),
    Value(String),
}

fn arrow_icon() -> &'static str {
    r#"<svg class="icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
  <path d="M5 12h12" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
  <path d="M13 6l6 6-6 6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"#
}

fn copy_icon() -> &'static str {
    r#"<svg class="icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
  <path d="M9 9h10v10H9V9Z" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>
  <path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
</svg>"#
}

fn render_app_page(csrf_field: &str, token: &str, result: Option<ResultBlock>) -> String {
    let result_block = match result {
        None => String::new(),
        Some(ResultBlock::Message(m)) => format!(
            r#"<div class="resultSurface" style="margin-top:14px;">
  <div class="pill">{}</div>
</div>"#,
            html_escape(&m)
        ),
        Some(ResultBlock::Value(v)) => format!(
            r#"<div class="resultSurface" style="margin-top:14px;">
  <pre id="resultValue" class="resultText">{}</pre>
  <button class="copyBtn" type="button" data-copy-target="resultValue" aria-label="Copier">
    {}
    <span class="srOnly">Copier</span>
  </button>
</div>"#,
            html_escape(&v),
            copy_icon()
        ),
    };

    let get_section = format!(
        r#"<section class="card">
  <div class="cardTitle">RETRIEVE</div>
  <form method="post" action="/get" class="fieldRow">
    {csrf}
    <label class="srOnly" for="get_key">Key</label>
    <input id="get_key" name="key" type="text" maxlength="255" placeholder="Key" autocomplete="off"/>
    <button class="actionBtn" type="submit" aria-label="Get">{arrow}</button>
  </form>
  {result_block}
</section>"#,
        csrf = csrf::hidden_input(csrf_field, token),
        arrow = arrow_icon(),
        result_block = result_block
    );

    let set_section = format!(
        r#"<section class="card">
  <div class="cardTitle">STORE</div>
  <form method="post" action="/set" class="fieldCol">
    {csrf}
    <label class="srOnly" for="set_key">Key</label>
    <input id="set_key" name="key" type="text" maxlength="255" placeholder="Key" autocomplete="off"/>

    <label class="srOnly" for="set_value">Value</label>
    <textarea id="set_value" name="value" maxlength="100000" placeholder="Content or Secret"></textarea>

    <div class="rowBetween">
      <label class="check">
        <input id="ephemeral" name="ephemeral" type="checkbox"/>
        <span class="muted" title="Content will be evaporated after the first reading">🌬️ EVAPORATING CONTENT</span>
      </label>
      <button class="actionBtn" type="submit" aria-label="Set">{arrow}</button>
    </div>
  </form>
</section>"#,
        csrf = csrf::hidden_input(csrf_field, token),
        arrow = arrow_icon()
    );

    base_page(
        "Vapor",
        &format!(
            r#"<div class="stack">
  <header>
    <div class="brand">VAPOR<span class="brandDot">-</span>LOCKER</div>
  </header>
  {get_section}
  <div class="divider" aria-hidden="true"></div>
  {set_section}
</div>"#,
            get_section = get_section,
            set_section = set_section
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

