//! Server configuration, read from `LOQ_*` environment variables.
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Serialize;

/// Who may create an account (`LOQ_REGISTRATION`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Registration {
    /// Anyone can register.
    Open,
    /// Only with a project invite link.
    Invite,
    /// Only admins create accounts.
    Closed,
}

impl std::str::FromStr for Registration {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "invite" => Ok(Self::Invite),
            "closed" => Ok(Self::Closed),
            other => bail!("LOQ_REGISTRATION must be open, invite or closed, not {other:?}"),
        }
    }
}

/// Requests allowed per key: a token bucket of `burst` tokens refilled one per
/// `refill`.
#[derive(Clone, Copy, Debug)]
pub struct RateLimit {
    pub burst: u32,
    pub refill: Duration,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    /// Where browsers reach the app, without a trailing slash. Used for links
    /// (invites, password resets), the `Origin` check and the cookie's
    /// `Secure` flag.
    pub public_url: String,
    pub registration: Registration,
    /// Take the client address from `X-Forwarded-For` (behind a reverse proxy)
    /// for rate limiting (`LOQ_TRUST_PROXY=1`).
    pub trust_proxy: bool,
    /// How long a room stays in memory after its last peer leaves.
    pub room_idle: Duration,
    /// How often a loaded room flushes its listing name and takes automatic
    /// versions.
    pub room_tick: Duration,
    /// Updates appended to the log before it is compacted into a snapshot.
    pub compact_every: u32,
    /// Minimum time between automatic versions of a project.
    pub auto_version_every: Duration,
    /// Rate limits on sign-in, registration and password reset, per client
    /// address and per account.
    pub limit_per_ip: RateLimit,
    pub limit_per_account: RateLimit,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.trim().is_empty());
        let mut config = Self::new(
            var("LOQ_DATA_DIR").map_or_else(|| PathBuf::from("./data"), PathBuf::from),
        );
        if let Some(bind) = var("LOQ_BIND") {
            config.bind = bind.parse().with_context(|| format!("invalid LOQ_BIND {bind:?}"))?;
        }
        if let Some(url) = var("LOQ_PUBLIC_URL") {
            config.public_url = url.trim().trim_end_matches('/').to_owned();
        }
        if let Some(registration) = var("LOQ_REGISTRATION") {
            config.registration = registration.parse()?;
        }
        config.trust_proxy = var("LOQ_TRUST_PROXY").is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"));
        Ok(config)
    }

    /// Development defaults with the given data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
            data_dir,
            public_url: "http://localhost:5173".into(),
            registration: Registration::Open,
            trust_proxy: false,
            room_idle: Duration::from_secs(60),
            room_tick: Duration::from_secs(5),
            compact_every: 500,
            auto_version_every: Duration::from_secs(3600),
            limit_per_ip: RateLimit {
                burst: 30,
                refill: Duration::from_secs(2),
            },
            limit_per_account: RateLimit {
                burst: 10,
                refill: Duration::from_secs(30),
            },
        }
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }

    /// `scheme://host[:port]` of the public URL, as browsers send in `Origin`.
    pub fn public_origin(&self) -> &str {
        let after_scheme = self.public_url.find("://").map_or(0, |i| i + 3);
        let end = self.public_url[after_scheme..]
            .find('/')
            .map_or(self.public_url.len(), |i| after_scheme + i);
        &self.public_url[..end]
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("loquaris.db")
    }

    pub fn blob_dir(&self) -> PathBuf {
        self.data_dir.join("blobs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_origin_strips_the_path() {
        let mut config = Config::new(PathBuf::from("x"));
        config.public_url = "https://example.org/loquaris".into();
        assert_eq!(config.public_origin(), "https://example.org");
        config.public_url = "http://localhost:5173".into();
        assert_eq!(config.public_origin(), "http://localhost:5173");
        assert!(!config.secure_cookies());
    }
}
