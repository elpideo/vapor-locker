//! Mécanismes de sécurité (limitation d’abus par IP).
//!
//! Chaque IP est associée à un score x ∈ [0, 16]. À chaque requête : x ← (x/2) * c^dt
//! (dt = temps depuis la dernière requête). Si x < 1 → 429 avec Retry-After.

use std::{net::IpAddr, time::Duration};

use moka::sync::Cache;

/// Constantes du système anti-abus.
const X_INITIAL: f64 = 16.0;
const X_MIN: f64 = 0.0;
const X_MAX: f64 = 16.0;
const X_THRESHOLD: f64 = 1.0;
const C_FACTOR: f64 = 1.1;

/// État par IP : score x et instant de la dernière requête.
#[derive(Clone, Copy)]
struct IpState {
    x: f64,
    last_seen: std::time::Instant,
}

/// Limiteur d’abus par IP (score décroissant à chaque requête, récupération dans le temps).
#[derive(Clone)]
pub struct AbuseLimiter {
    cache: Cache<IpAddr, IpState>,
}

impl AbuseLimiter {
    /// Crée un limiteur avec TTL d’entrée (après lequel l’IP est oubliée et repart à 16).
    ///
    /// Le facteur `c` est une constante interne (`C_FACTOR`).
    pub fn new(ttl: Duration) -> Self {
        assert!(C_FACTOR > 1.0, "C_FACTOR must be > 1");
        Self {
            cache: Cache::builder()
                .time_to_live(ttl)
                .max_capacity(100_000)
                .build(),
        }
    }

    /// Vérifie l’IP et met à jour le score. Retourne `Ok(())` si la requête est autorisée,
    /// ou `Err(retry_after_secs)` si 429 doit être renvoyé (avec en-tête Retry-After).
    pub fn check_or_update(&self, ip: IpAddr) -> Result<(), u64> {
        let now = std::time::Instant::now();
        let (x_prev, t_prev) = self
            .cache
            .get(&ip)
            .map(|s| (s.x, s.last_seen))
            .unwrap_or((X_INITIAL, now));

        let dt_secs = now.duration_since(t_prev).as_secs_f64();
        let x_new = (x_prev / 2.0) * C_FACTOR.powf(dt_secs);
        let x_new = x_new.clamp(X_MIN, X_MAX);

        self.cache.insert(
            ip,
            IpState {
                x: x_new,
                last_seen: now,
            },
        );

        if x_new < X_THRESHOLD {
            let retry_secs = (2.0_f64.ln() - x_new.ln()) / C_FACTOR.ln();
            let retry_secs = retry_secs.ceil().max(1.0) as u64;
            return Err(retry_secs);
        }

        
        Ok(())
    }
}
