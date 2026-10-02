//! Platform adapters. They only know upstream protocol details; profile and
//! aggregation policy belongs to the backend crate.

mod http;
pub mod kugou;
pub mod netease;
pub mod spotify;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;
use yaqmc_music_providers_protocol::{
    AccountSnapshot, AuthMethod, LibrarySnapshot, Lyrics, Platform, PlaybackSource, SearchResult,
};

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("invalid platform configuration: {0}")]
    Configuration(String),
    #[error("authentication is required")]
    AuthenticationRequired,
    #[error("authentication is still pending")]
    AuthenticationPending,
    #[error("upstream service returned an invalid response")]
    InvalidResponse,
    #[error("upstream service request failed: {0}")]
    Upstream(String),
    #[error("operation is not supported by this platform")]
    Unsupported,
    #[error("full-track playback is not available through this platform API")]
    FullPlaybackUnavailable,
}

#[derive(Clone, Debug)]
pub struct ProfileSession {
    pub platform: Platform,
    pub profile_id: String,
    pub secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at_ms: Option<u64>,
}

impl ProfileSession {
    pub fn guest(platform: Platform, profile_id: impl Into<String>) -> Self {
        Self {
            platform,
            profile_id: profile_id.into(),
            secret: None,
            access_token: None,
            refresh_token: None,
            expires_at_ms: None,
        }
    }

    pub fn authenticated(&self) -> bool {
        self.secret.is_some() || self.access_token.is_some()
    }

    pub fn bearer(&self) -> Result<&str, PlatformError> {
        self.access_token
            .as_deref()
            .ok_or(PlatformError::AuthenticationRequired)
    }

    pub fn cookie(&self) -> Result<&str, PlatformError> {
        self.secret
            .as_deref()
            .ok_or(PlatformError::AuthenticationRequired)
    }
}

#[derive(Clone, Debug, Default)]
pub struct AuthPreparation {
    pub attempt_id: String,
    pub authorization_url: Option<String>,
    pub qr_payload: Option<String>,
    pub expires_at_ms: u64,
    pub verifier: Option<String>,
    pub state: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AuthCompletion {
    pub secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at_ms: Option<u64>,
    pub display_name: Option<String>,
    pub entitlement: Option<String>,
}

#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn platform(&self) -> Platform;
    fn auth_methods(&self) -> Vec<AuthMethod>;
    async fn prepare_auth(
        &self,
        session: &ProfileSession,
        method: &str,
    ) -> Result<AuthPreparation, PlatformError>;
    async fn complete_auth(
        &self,
        session: &ProfileSession,
        preparation: &AuthPreparation,
        payload: &Value,
    ) -> Result<AuthCompletion, PlatformError>;
    async fn account_snapshot(
        &self,
        session: &ProfileSession,
    ) -> Result<AccountSnapshot, PlatformError>;
    async fn search(
        &self,
        session: &ProfileSession,
        query: &str,
        page: u32,
        limit: u32,
    ) -> Result<SearchResult, PlatformError>;
    async fn library(
        &self,
        session: &ProfileSession,
        limit: u32,
    ) -> Result<LibrarySnapshot, PlatformError>;
    async fn lyrics(
        &self,
        session: &ProfileSession,
        track_id: &str,
    ) -> Result<Lyrics, PlatformError>;
    async fn playback(
        &self,
        session: &ProfileSession,
        track_id: &str,
        quality: Option<&str>,
    ) -> Result<PlaybackSource, PlatformError>;
}

pub(crate) fn require_non_empty(value: &str, field: &str) -> Result<(), PlatformError> {
    if value.trim().is_empty() {
        return Err(PlatformError::Configuration(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

pub(crate) fn bounded_limit(value: u32) -> u32 {
    value.clamp(1, 100)
}

pub(crate) fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
