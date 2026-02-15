//! Protection CSRF (Cross-Site Request Forgery).
//!
//! Gère la génération, la distribution et la vérification des tokens CSRF
//! via cookie et champ caché dans les formulaires.

use axum::{http::StatusCode, response::Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use tower_cookies::{Cookie, Cookies};

/// Configuration du mécanisme CSRF.
#[derive(Clone)]
pub struct CsrfConfig {
    pub cookie_name: String,
    pub field_name: String,
    pub secure_cookie: bool,
}

impl CsrfConfig {
    /// Crée la configuration à partir des variables d'environnement.
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

    /// Émet un token CSRF et le stocke dans un cookie. Réutilise le token existant si présent.
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

    /// Vérifie que le token soumis correspond à celui du cookie. Retourne une erreur 403 si invalide.
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

/// Génère le HTML d'un champ caché pour inclure le token CSRF dans un formulaire.
pub fn hidden_input(field_name: &str, token: &str) -> String {
    format!(
        r#"<input type="hidden" name="{field_name}" value="{token}"/>"#,
        field_name = html_escape(field_name),
        token = html_escape(token)
    )
}

/// Échappe les caractères HTML pour éviter les injections XSS.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Convertit un code HTTP en réponse Axum.
fn status_response(code: StatusCode) -> Response {
    axum::response::IntoResponse::into_response(code)
}

/// Extension pour convertir un `StatusCode` en `Response`.
trait IntoResponseExt {
    fn into_response(self) -> Response;
}

impl IntoResponseExt for StatusCode {
    fn into_response(self) -> Response {
        status_response(self)
    }
}

