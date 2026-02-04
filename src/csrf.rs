use axum::{http::StatusCode, response::Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use tower_cookies::{Cookie, Cookies};

#[derive(Clone)]
pub struct CsrfConfig {
    pub cookie_name: String,
    pub field_name: String,
    pub secure_cookie: bool,
}

impl CsrfConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let secure_cookie = std::env::var("COOKIE_SECURE")
            .unwrap_or_else(|_| "false".to_string())
            .to_lowercase()
            == "true";

        Ok(Self {
            cookie_name: std::env::var("CSRF_COOKIE_NAME").unwrap_or_else(|_| "vapor_csrf".into()),
            field_name: std::env::var("CSRF_FIELD_NAME").unwrap_or_else(|_| "csrf".into()),
            secure_cookie,
        })
    }

    pub fn issue_token(&self, cookies: &Cookies) -> anyhow::Result<String> {
        if let Some(existing) = cookies.get(&self.cookie_name) {
            let v = existing.value().to_string();
            if !v.is_empty() {
                return Ok(v);
            }
        }

        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);

        let mut c = Cookie::new(self.cookie_name.clone(), token.clone());
        c.set_http_only(true);
        c.set_same_site(cookie::SameSite::Strict);
        c.set_path("/");
        c.set_secure(self.secure_cookie);
        cookies.add(c);

        Ok(token)
    }

    pub fn verify(&self, cookies: &Cookies, submitted: Option<&str>) -> Result<(), Response> {
        let cookie_val = cookies
            .get(&self.cookie_name)
            .map(|c| c.value().to_string());
        let Some(cookie_val) = cookie_val else {
            return Err(StatusCode::FORBIDDEN.into_response());
        };
        let Some(submitted) = submitted else {
            return Err(StatusCode::FORBIDDEN.into_response());
        };
        if cookie_val != submitted {
            return Err(StatusCode::FORBIDDEN.into_response());
        }
        Ok(())
    }
}

pub fn hidden_input(field_name: &str, token: &str) -> String {
    format!(
        r#"<input type="hidden" name="{field_name}" value="{token}"/>"#,
        field_name = html_escape(field_name),
        token = html_escape(token)
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn status_response(code: StatusCode) -> Response {
    axum::response::IntoResponse::into_response(code)
}

trait IntoResponseExt {
    fn into_response(self) -> Response;
}

impl IntoResponseExt for StatusCode {
    fn into_response(self) -> Response {
        status_response(self)
    }
}

