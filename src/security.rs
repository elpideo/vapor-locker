use std::{net::IpAddr, time::Duration};

use moka::sync::Cache;

#[derive(Clone)]
pub struct IpCache {
    cache: Cache<IpAddr, ()>,
}

impl IpCache {
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

