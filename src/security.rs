//! Mécanismes de sécurité (rate limiting par IP).
//!
//! Cache des adresses IP récemment vues pour limiter les requêtes répétées.

use std::{net::IpAddr, time::Duration};

use moka::sync::Cache;

/// Cache des adresses IP avec TTL pour détecter les requêtes répétées.
#[derive(Clone)]
pub struct IpCache {
    cache: Cache<IpAddr, ()>,
}

impl IpCache {
    /// Crée un cache avec une durée de vie (TTL) et une capacité max de 100 000 entrées.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Cache::builder().time_to_live(ttl).max_capacity(100_000).build(),
        }
    }

    /// Returns true if the IP was already in cache (i.e. should short-circuit).
    pub fn seen_recently(&self, ip: IpAddr) -> bool {
        if self.cache.get(&ip).is_some() {
            return true;
        }
        self.cache.insert(ip, ());
        false
    }
}

