//! In-memory token buckets for sign-in, registration and password reset,
//! keyed by client address and by account. One process holds everything,
//! so no shared store is needed.
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::AppState;
use crate::config::RateLimit;
use crate::error::ApiError;

struct Bucket {
    tokens: f64,
    updated: Instant,
}

#[derive(Default)]
pub struct Limiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl Limiter {
    /// Takes a token from `key`'s bucket; false if none is left.
    pub fn check(&self, key: &str, limit: RateLimit) -> bool {
        let now = Instant::now();
        let burst = f64::from(limit.burst);
        let refill = |bucket: &Bucket| {
            let elapsed = now.duration_since(bucket.updated).as_secs_f64();
            (bucket.tokens + elapsed / limit.refill.as_secs_f64().max(1e-3)).min(burst)
        };
        let mut buckets = self.buckets.lock().expect("rate limiter poisoned");
        if buckets.len() > 10_000 {
            // Forget full buckets; they behave exactly like missing ones.
            buckets.retain(|_, bucket| refill(bucket) < burst);
        }
        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
            tokens: burst,
            updated: now,
        });
        bucket.tokens = refill(bucket);
        bucket.updated = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Checks the per-address and per-account limits of an action.
    pub fn check_both(
        &self,
        state: &AppState,
        action: &str,
        ip: ClientIp,
        account: Option<&str>,
    ) -> Result<(), ApiError> {
        let ip_ok = self.check(&format!("{action}:ip:{}", ip.0), state.config.limit_per_ip);
        let account_ok = account.is_none_or(|account| {
            self.check(&format!("{action}:account:{account}"), state.config.limit_per_account)
        });
        if ip_ok && account_ok { Ok(()) } else { Err(ApiError::TooManyRequests) }
    }
}

/// The client's address: the peer address, or the last `X-Forwarded-For` hop
/// when `LOQ_TRUST_PROXY` says a reverse proxy sets it.
#[derive(Clone, Copy, Debug)]
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let forwarded = state
            .config
            .trust_proxy
            .then(|| parts.headers.get("x-forwarded-for")?.to_str().ok()?.rsplit(',').next()?.trim().parse().ok())
            .flatten();
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        Ok(Self(forwarded.or(peer).unwrap_or(IpAddr::from([0, 0, 0, 0]))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn buckets_drain_and_refill() {
        let limiter = Limiter::default();
        let limit = RateLimit {
            burst: 3,
            refill: Duration::from_millis(50),
        };
        assert!((0..3).all(|_| limiter.check("k", limit)));
        assert!(!limiter.check("k", limit));
        assert!(limiter.check("other", limit));
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.check("k", limit));
    }
}
