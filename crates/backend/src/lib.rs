//! Local coordinator for the independent YAQMC music providers.

use axum::{
    body::Body,
    extract::Path as AxumPath,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Json, Router,
};
use futures::{future::join_all, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Mutex, RwLock},
};
use url::Url;
use uuid::Uuid;
use yaqmc_music_providers_platforms::{
    kugou::{KugouClient, KugouConfig},
    netease::{NeteaseClient, NeteaseConfig},
    spotify::{SpotifyClient, SpotifyConfig},
    AuthCompletion, AuthPreparation, PlatformAdapter, PlatformError, ProfileSession,
};
use yaqmc_music_providers_protocol::{
    AuthAttempt, LibrarySnapshot, Platform, ProtocolError, Request, Response, SearchResult,
    UseMode, DEFAULT_BACKEND_PORT, PROTOCOL_VERSION,
};

const DEFAULT_CONFIG_FILE: &str = "config.toml";
const DEFAULT_DATA_DIR: &str = "data";
const DEFAULT_NETEASE_API: &str = "https://music.163.com";
const MAX_LINE_BYTES: usize = 128 * 1024;
const MEDIA_TTL_MS: u64 = 10 * 60 * 1000;
const MAX_MEDIA_ENTRIES: usize = 1024;
const AUTH_ATTEMPT_RETENTION_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("state could not be read")]
    StateRead,
    #[error("state could not be written")]
    StateWrite,
    #[error("backend request is invalid: {0}")]
    InvalidRequest(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: DEFAULT_BACKEND_PORT,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifySettings {
    #[serde(default)]
    pub client_id: String,
    #[serde(default = "default_spotify_redirect")]
    pub redirect_uri: String,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub accounts_base_url: Option<String>,
}

fn default_spotify_redirect() -> String {
    "http://127.0.0.1:43821/oauth/spotify/callback".to_owned()
}

impl Default for SpotifySettings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            redirect_uri: default_spotify_redirect(),
            api_base_url: None,
            accounts_base_url: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSettings {
    #[serde(default)]
    pub base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub mode: UseMode,
    #[serde(default)]
    pub spotify: SpotifySettings,
    #[serde(default)]
    pub netease: EndpointSettings,
    #[serde(default)]
    pub kugou: EndpointSettings,
    #[serde(default)]
    pub http_auth_token: Option<String>,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(DEFAULT_DATA_DIR)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen: ListenConfig::default(),
            data_dir: default_data_dir(),
            mode: UseMode::Isolated,
            spotify: SpotifySettings::default(),
            netease: EndpointSettings {
                base_url: DEFAULT_NETEASE_API.to_owned(),
            },
            kugou: EndpointSettings::default(),
            http_auth_token: None,
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, BackendError> {
        let mut config = if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|_| BackendError::StateRead)?;
            toml::from_str(&text).map_err(|error| BackendError::Configuration(error.to_string()))?
        } else {
            Self::default()
        };
        if let Ok(client_id) = std::env::var("SPOTIFY_CLIENT_ID") {
            if !client_id.trim().is_empty() {
                config.spotify.client_id = client_id;
            }
        }
        if let Ok(base_url) = std::env::var("NETEASE_API_BASE_URL") {
            if !base_url.trim().is_empty() {
                config.netease.base_url = base_url;
            }
        }
        if let Ok(base_url) = std::env::var("KUGOU_API_BASE_URL") {
            if !base_url.trim().is_empty() {
                config.kugou.base_url = base_url;
            }
        }
        Ok(config)
    }

    pub fn write_template(path: &Path) -> Result<(), BackendError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| BackendError::StateWrite)?;
        }
        let text = toml::to_string_pretty(&Self::default())
            .map_err(|error| BackendError::Configuration(error.to_string()))?;
        std::fs::write(path, text).map_err(|_| BackendError::StateWrite)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct StoredState {
    profiles: HashMap<String, StoredProfile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredProfile {
    platform: Platform,
    profile_id: String,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

impl StoredProfile {
    fn session(&self) -> ProfileSession {
        ProfileSession {
            platform: self.platform,
            profile_id: self.profile_id.clone(),
            secret: self.secret.clone(),
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
            expires_at_ms: self.expires_at_ms,
        }
    }
}

struct StateStore {
    path: PathBuf,
    state: Mutex<StoredState>,
}

impl StateStore {
    fn load(data_dir: &Path) -> Result<Self, BackendError> {
        std::fs::create_dir_all(data_dir).map_err(|_| BackendError::StateWrite)?;
        let path = data_dir.join("profiles.json");
        let state = if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(|_| BackendError::StateRead)?;
            serde_json::from_str(&text).map_err(|_| BackendError::StateRead)?
        } else {
            StoredState::default()
        };
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }

    async fn session(&self, platform: Platform, profile_id: &str) -> ProfileSession {
        let key = profile_key(platform, profile_id);
        self.state
            .lock()
            .await
            .profiles
            .get(&key)
            .map(StoredProfile::session)
            .unwrap_or_else(|| ProfileSession::guest(platform, profile_id))
    }

    async fn put(
        &self,
        completion: &AuthCompletion,
        session: &ProfileSession,
    ) -> Result<(), BackendError> {
        let key = profile_key(session.platform, &session.profile_id);
        let profile = StoredProfile {
            platform: session.platform,
            profile_id: session.profile_id.clone(),
            secret: completion.secret.clone().or_else(|| session.secret.clone()),
            access_token: completion
                .access_token
                .clone()
                .or_else(|| session.access_token.clone()),
            refresh_token: completion
                .refresh_token
                .clone()
                .or_else(|| session.refresh_token.clone()),
            expires_at_ms: completion.expires_at_ms.or(session.expires_at_ms),
        };
        let mut state = self.state.lock().await;
        state.profiles.insert(key, profile);
        self.flush_locked(&state)
    }

    async fn remove(&self, platform: Platform, profile_id: &str) -> Result<(), BackendError> {
        let mut state = self.state.lock().await;
        state.profiles.remove(&profile_key(platform, profile_id));
        self.flush_locked(&state)
    }

    fn flush_locked(&self, state: &StoredState) -> Result<(), BackendError> {
        let temp = self.path.with_extension("json.tmp");
        let text = serde_json::to_vec_pretty(state).map_err(|_| BackendError::StateWrite)?;
        std::fs::write(&temp, text).map_err(|_| BackendError::StateWrite)?;
        std::fs::rename(temp, &self.path).map_err(|_| BackendError::StateWrite)
    }
}

#[derive(Clone)]
struct AuthAttemptState {
    platform: Platform,
    profile_id: String,
    preparation: AuthPreparation,
    outcome: AuthAttemptOutcome,
    retained_until_ms: u64,
}

#[derive(Clone)]
enum AuthAttemptOutcome {
    Active,
    Completed(Value),
    Expired,
    Cancelled,
}

#[derive(Clone)]
struct MediaEntry {
    upstream_url: String,
    content_length: u64,
    mime_type: String,
    expires_at_ms: u64,
}

#[derive(Clone)]
pub struct Backend {
    config: Arc<RwLock<AppConfig>>,
    store: Arc<StateStore>,
    adapters: Arc<HashMap<Platform, Arc<dyn PlatformAdapter>>>,
    attempts: Arc<Mutex<HashMap<String, AuthAttemptState>>>,
    media: Arc<Mutex<HashMap<String, MediaEntry>>>,
    media_client: reqwest::Client,
}

impl Backend {
    pub fn new(config: AppConfig) -> Result<Self, BackendError> {
        let mut adapters: HashMap<Platform, Arc<dyn PlatformAdapter>> = HashMap::new();
        if !config.spotify.client_id.trim().is_empty() {
            if config.spotify.redirect_uri != default_spotify_redirect() {
                return Err(BackendError::Configuration(
                    "Spotify redirect URI must be http://127.0.0.1:43821/oauth/spotify/callback"
                        .to_owned(),
                ));
            }
            let mut spotify = SpotifyConfig::from_client_id(config.spotify.client_id.clone());
            spotify.redirect_uri = config.spotify.redirect_uri.clone();
            if let Some(value) = &config.spotify.accounts_base_url {
                spotify.accounts_base_url = value.clone();
            }
            if let Some(value) = &config.spotify.api_base_url {
                spotify.api_base_url = value.clone();
            }
            let client = SpotifyClient::new(spotify)
                .map_err(|error| BackendError::Configuration(error.to_string()))?;
            adapters.insert(Platform::Spotify, Arc::new(client));
        }
        if !config.netease.base_url.trim().is_empty() {
            let client = NeteaseClient::new(NeteaseConfig {
                base_url: config.netease.base_url.clone(),
            })
            .map_err(|error| BackendError::Configuration(error.to_string()))?;
            adapters.insert(Platform::Netease, Arc::new(client));
        }
        if !config.kugou.base_url.trim().is_empty() {
            let client = KugouClient::new(KugouConfig {
                base_url: config.kugou.base_url.clone(),
            })
            .map_err(|error| BackendError::Configuration(error.to_string()))?;
            adapters.insert(Platform::Kugou, Arc::new(client));
        }
        let store = StateStore::load(&config.data_dir)?;
        let media_client = reqwest::Client::builder()
            .user_agent("yaqmc-music-providers/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| BackendError::Configuration("media client could not start".to_owned()))?;
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            store: Arc::new(store),
            adapters: Arc::new(adapters),
            attempts: Arc::new(Mutex::new(HashMap::new())),
            media: Arc::new(Mutex::new(HashMap::new())),
            media_client,
        })
    }

    pub async fn handle(&self, request: Request) -> Response {
        let id = request.id.clone();
        let result = self.dispatch(request).await;
        match result {
            Ok(value) => Response {
                id,
                version: PROTOCOL_VERSION,
                ok: true,
                result: Some(value),
                error: None,
            },
            Err(error) => Response {
                id,
                version: PROTOCOL_VERSION,
                ok: false,
                result: None,
                error: Some(error),
            },
        }
    }

    async fn dispatch(&self, request: Request) -> Result<Value, ProtocolError> {
        if request.version != PROTOCOL_VERSION {
            return Err(ProtocolError::invalid_request(
                "unsupported provider protocol version",
            ));
        }
        if request.id.trim().is_empty() || request.id.len() > 128 {
            return Err(ProtocolError::invalid_request("request id is invalid"));
        }
        if !matches!(request.operation.as_str(), "health" | "provider.list") {
            profile_id(&request)?;
        }
        match request.operation.as_str() {
            "health" => self.health().await,
            "provider.list" => self.provider_list().await,
            "auth.login-methods" => self.login_methods(&request).await,
            "auth.prepare" => self.prepare_auth(&request).await,
            "auth.complete" => self.complete_auth(&request).await,
            "auth.cancel" => self.cancel_auth(&request).await,
            "auth.refresh" => self.refresh_auth(&request).await,
            "auth.logout" => self.logout(&request).await,
            "account.snapshot" => self.account_snapshot(&request).await,
            "catalog.search" | "aggregate.search" => self.search(&request).await,
            "library.snapshot" | "aggregate.library" => self.library(&request).await,
            "lyrics.get" => self.lyrics(&request).await,
            "playback.resolve" => self.playback(&request).await,
            _ => Err(ProtocolError::unsupported(
                "the backend operation is not declared",
            )),
        }
    }

    async fn health(&self) -> Result<Value, ProtocolError> {
        let config = self.config.read().await;
        Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "mode": config.mode,
            "platforms": self.adapters.keys().map(|platform| platform.as_str()).collect::<Vec<_>>(),
            "qqmusic": "host-delegated",
        }))
    }

    async fn provider_list(&self) -> Result<Value, ProtocolError> {
        let mut providers = Vec::new();
        for platform in Platform::ALL_EXTERNAL {
            if let Some(adapter) = self.adapters.get(&platform) {
                providers.push(json!({
                    "platform": platform,
                    "configured": true,
                    "authMethods": adapter.auth_methods(),
                }));
            } else {
                providers.push(json!({
                    "platform": platform,
                    "configured": false,
                    "authMethods": [],
                }));
            }
        }
        providers.push(json!({
            "platform": Platform::Qqmusic,
            "configured": true,
            "delegatedTo": "yaqmc-host",
        }));
        Ok(json!({ "providers": providers }))
    }

    async fn login_methods(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let adapter = self.adapter(platform)?;
        Ok(json!({ "platform": platform, "methods": adapter.auth_methods() }))
    }

    async fn prepare_auth(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let profile_id = profile_id(request)?;
        let method = request
            .payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("qr");
        let state = request.payload.get("state").and_then(Value::as_str);
        let attempt_id = request.payload.get("attemptId").and_then(Value::as_str);
        let response = self
            .prepare_auth_attempt(platform, profile_id, method, state, attempt_id)
            .await?;
        serde_json::to_value(response).map_err(|_| ProtocolError::internal())
    }

    async fn complete_auth(&self, request: &Request) -> Result<Value, ProtocolError> {
        let attempt_id = request
            .payload
            .get("attemptId")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::invalid_request("attemptId is required"))?;
        validate_attempt_id(attempt_id)?;
        let attempt = self.auth_attempt(attempt_id).await?;
        if let AuthAttemptOutcome::Completed(snapshot) = &attempt.outcome {
            return Ok(snapshot.clone());
        }
        if matches!(&attempt.outcome, AuthAttemptOutcome::Cancelled) {
            return Err(ProtocolError::invalid_request(
                "authentication attempt was cancelled",
            ));
        }
        if matches!(&attempt.outcome, AuthAttemptOutcome::Expired)
            || attempt.preparation.expires_at_ms <= now_ms()
        {
            self.mark_attempt_expired(attempt_id).await;
            if is_qr_attempt(&attempt) {
                return Ok(auth_attempt_status(&attempt, "expired"));
            }
            return Err(ProtocolError::invalid_request(
                "authentication attempt has expired",
            ));
        }
        let adapter = self.adapter(attempt.platform)?;
        let session = self
            .store
            .session(attempt.platform, &attempt.profile_id)
            .await;
        let mut payload = request.payload.clone();
        if payload.get("code").is_none() {
            if let Some(callback) = payload.get("callbackUrl").and_then(Value::as_str) {
                if let Ok(url) = Url::parse(callback) {
                    if let Some((_, code)) = url.query_pairs().find(|(key, _)| key == "code") {
                        payload["code"] = Value::String(code.into_owned());
                    }
                }
            }
        }
        if let Some(expected_state) = attempt.preparation.state.as_deref() {
            let actual_state = payload
                .get("state")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    payload
                        .get("callbackUrl")
                        .and_then(Value::as_str)
                        .and_then(|callback| callback_query_value(callback, "state"))
                });
            if actual_state.as_deref() != Some(expected_state) {
                return Err(ProtocolError::invalid_request(
                    "authentication callback state is invalid",
                ));
            }
        }
        let completion = match adapter
            .complete_auth(&session, &attempt.preparation, &payload)
            .await
        {
            Ok(completion) => completion,
            Err(PlatformError::AuthenticationPending) if is_qr_attempt(&attempt) => {
                return Ok(auth_attempt_status(&attempt, "pending"));
            }
            Err(PlatformError::Upstream(message))
                if is_qr_attempt(&attempt) && message.to_ascii_lowercase().contains("expired") =>
            {
                self.mark_attempt_expired(attempt_id).await;
                return Ok(auth_attempt_status(&attempt, "expired"));
            }
            Err(error) => return Err(map_platform_error(error)),
        };
        self.store
            .put(&completion, &session)
            .await
            .map_err(|_| ProtocolError::internal())?;
        let snapshot = adapter
            .account_snapshot(&session_with_completion(&session, &completion))
            .await
            .map_err(map_platform_error)?;
        let snapshot = serde_json::to_value(snapshot).map_err(|_| ProtocolError::internal())?;
        self.mark_attempt_completed(attempt_id, snapshot.clone())
            .await?;
        Ok(snapshot)
    }

    async fn cancel_auth(&self, request: &Request) -> Result<Value, ProtocolError> {
        let attempt_id = request
            .payload
            .get("attemptId")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::invalid_request("attemptId is required"))?;
        validate_attempt_id(attempt_id)?;
        let flow = request
            .payload
            .get("flow")
            .and_then(Value::as_str)
            .unwrap_or("qr");
        if let Ok(attempt) = self.auth_attempt(attempt_id).await {
            if let AuthAttemptOutcome::Completed(snapshot) = &attempt.outcome {
                return Ok(snapshot.clone());
            }
            self.mark_attempt_cancelled(attempt_id).await;
            if flow == "qr" {
                return Ok(auth_attempt_status(&attempt, "cancelled"));
            }
            return self
                .current_account_snapshot(attempt.platform, &attempt.profile_id)
                .await;
        }
        if flow == "qr" {
            let platform = required_platform(request)?;
            let profile_id = profile_id(request)?;
            return Ok(json!({
                "status": "cancelled",
                "attemptId": attempt_id,
                "platform": platform,
                "profileId": profile_id,
                "qrPayload": null,
                "expiresAtMs": 0
            }));
        }
        let platform = required_platform(request)?;
        let profile_id = profile_id(request)?;
        self.current_account_snapshot(platform, &profile_id).await
    }

    async fn refresh_auth(&self, request: &Request) -> Result<Value, ProtocolError> {
        let old_attempt_id = request
            .payload
            .get("attemptId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let (platform, profile_id) = if let Some(attempt_id) = old_attempt_id {
            let attempt = self.auth_attempt(attempt_id).await?;
            if !is_qr_attempt(&attempt) {
                return Err(ProtocolError::unsupported(
                    "only QR authentication attempts can be refreshed",
                ));
            }
            self.attempts.lock().await.remove(attempt_id);
            (attempt.platform, attempt.profile_id)
        } else {
            (required_platform(request)?, profile_id(request)?)
        };
        let response = self
            .prepare_auth_attempt(platform, profile_id, "qr", None, None)
            .await?;
        serde_json::to_value(response).map_err(|_| ProtocolError::internal())
    }

    async fn prepare_auth_attempt(
        &self,
        platform: Platform,
        profile_id: String,
        method: &str,
        state: Option<&str>,
        requested_attempt_id: Option<&str>,
    ) -> Result<AuthAttempt, ProtocolError> {
        let adapter = self.adapter(platform)?;
        let session = self.store.session(platform, &profile_id).await;
        let mut preparation = adapter
            .prepare_auth(&session, method)
            .await
            .map_err(map_platform_error)?;
        if let Some(state) = state {
            validate_oauth_state(state)?;
            if let Some(authorization_url) = preparation.authorization_url.as_mut() {
                *authorization_url = authorization_url_with_state(authorization_url, state)?;
            }
            preparation.state = Some(state.to_owned());
        }
        if let Some(attempt_id) = requested_attempt_id {
            validate_attempt_id(attempt_id)?;
            preparation.attempt_id = attempt_id.to_owned();
        }
        let response = AuthAttempt {
            attempt_id: preparation.attempt_id.clone(),
            platform,
            profile_id: profile_id.clone(),
            authorization_url: preparation.authorization_url.clone(),
            qr_payload: preparation.qr_payload.clone(),
            expires_at_ms: preparation.expires_at_ms,
        };
        let retained_until_ms = preparation
            .expires_at_ms
            .saturating_add(AUTH_ATTEMPT_RETENTION_MS);
        let mut attempts = self.attempts.lock().await;
        prune_attempts(&mut attempts);
        attempts.insert(
            response.attempt_id.clone(),
            AuthAttemptState {
                platform,
                profile_id,
                preparation,
                outcome: AuthAttemptOutcome::Active,
                retained_until_ms,
            },
        );
        Ok(response)
    }

    async fn auth_attempt(&self, attempt_id: &str) -> Result<AuthAttemptState, ProtocolError> {
        let mut attempts = self.attempts.lock().await;
        prune_attempts(&mut attempts);
        attempts
            .get(attempt_id)
            .cloned()
            .ok_or_else(|| ProtocolError::invalid_request("authentication attempt is unknown"))
    }

    async fn mark_attempt_expired(&self, attempt_id: &str) {
        self.update_attempt(attempt_id, |attempt| {
            attempt.outcome = AuthAttemptOutcome::Expired;
            attempt.retained_until_ms = now_ms().saturating_add(AUTH_ATTEMPT_RETENTION_MS);
        })
        .await;
    }

    async fn mark_attempt_cancelled(&self, attempt_id: &str) {
        self.update_attempt(attempt_id, |attempt| {
            attempt.outcome = AuthAttemptOutcome::Cancelled;
            attempt.retained_until_ms = now_ms().saturating_add(AUTH_ATTEMPT_RETENTION_MS);
        })
        .await;
    }

    async fn mark_attempt_completed(
        &self,
        attempt_id: &str,
        snapshot: Value,
    ) -> Result<(), ProtocolError> {
        let mut attempts = self.attempts.lock().await;
        let attempt = attempts
            .get_mut(attempt_id)
            .ok_or_else(|| ProtocolError::invalid_request("authentication attempt is unknown"))?;
        attempt.outcome = AuthAttemptOutcome::Completed(snapshot);
        attempt.retained_until_ms = now_ms().saturating_add(AUTH_ATTEMPT_RETENTION_MS);
        Ok(())
    }

    async fn update_attempt<F>(&self, attempt_id: &str, update: F)
    where
        F: FnOnce(&mut AuthAttemptState),
    {
        if let Some(attempt) = self.attempts.lock().await.get_mut(attempt_id) {
            update(attempt);
        }
    }

    async fn current_account_snapshot(
        &self,
        platform: Platform,
        profile_id: &str,
    ) -> Result<Value, ProtocolError> {
        let adapter = self.adapter(platform)?;
        let session = self.store.session(platform, profile_id).await;
        serde_json::to_value(
            adapter
                .account_snapshot(&session)
                .await
                .map_err(map_platform_error)?,
        )
        .map_err(|_| ProtocolError::internal())
    }

    async fn logout(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let profile_id = profile_id(request)?;
        self.store
            .remove(platform, &profile_id)
            .await
            .map_err(|_| ProtocolError::internal())?;
        self.attempts
            .lock()
            .await
            .retain(|_, attempt| attempt.platform != platform || attempt.profile_id != profile_id);
        Ok(json!({
            "platform": platform,
            "profileId": profile_id,
            "authenticated": false
        }))
    }

    async fn account_snapshot(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let adapter = self.adapter(platform)?;
        let profile_id = profile_id(request)?;
        let session = self.store.session(platform, &profile_id).await;
        serde_json::to_value(
            adapter
                .account_snapshot(&session)
                .await
                .map_err(map_platform_error)?,
        )
        .map_err(|_| ProtocolError::internal())
    }

    async fn search(&self, request: &Request) -> Result<Value, ProtocolError> {
        let query = request
            .payload
            .get("query")
            .and_then(Value::as_str)
            .filter(|query| !query.trim().is_empty())
            .ok_or_else(|| ProtocolError::invalid_request("query is required"))?;
        let page = request
            .payload
            .get("page")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let limit = request
            .payload
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(30) as u32;
        if should_aggregate(request) {
            let results = join_all(
                Platform::ALL_EXTERNAL
                    .into_iter()
                    .filter(|platform| self.adapters.contains_key(platform))
                    .map(|platform| self.search_one(platform, request, query, page, limit)),
            )
            .await;
            let mut items = Vec::new();
            let mut warnings = Vec::new();
            for result in results {
                match result {
                    Ok(value) => {
                        items.extend(value.items);
                        warnings.extend(value.warnings);
                    }
                    Err(error) => warnings.push(error.message),
                }
            }
            return serde_json::to_value(SearchResult {
                items,
                next_cursor: None,
                platform: None,
                warnings,
            })
            .map_err(|_| ProtocolError::internal());
        }
        let platform = required_platform(request)?;
        serde_json::to_value(
            self.search_one(platform, request, query, page, limit)
                .await?,
        )
        .map_err(|_| ProtocolError::internal())
    }

    async fn search_one(
        &self,
        platform: Platform,
        request: &Request,
        query: &str,
        page: u32,
        limit: u32,
    ) -> Result<SearchResult, ProtocolError> {
        let adapter = self.adapter(platform)?;
        let session = self
            .store
            .session(platform, &profile_id_or_default(request))
            .await;
        adapter
            .search(&session, query, page, limit)
            .await
            .map_err(map_platform_error)
    }

    async fn library(&self, request: &Request) -> Result<Value, ProtocolError> {
        if should_aggregate(request) {
            let results = join_all(
                Platform::ALL_EXTERNAL
                    .into_iter()
                    .filter(|platform| self.adapters.contains_key(platform))
                    .map(|platform| self.library_one(platform, request)),
            )
            .await;
            let mut tracks = Vec::new();
            let mut playlists = Vec::new();
            let mut warnings = Vec::new();
            for result in results {
                match result {
                    Ok(value) => {
                        tracks.extend(value.tracks);
                        playlists.extend(value.playlists);
                        warnings.extend(value.warnings);
                    }
                    Err(error) => warnings.push(error.message),
                }
            }
            return serde_json::to_value(json!({
                "mode": "aggregate",
                "tracks": tracks,
                "playlists": playlists,
                "warnings": warnings,
                "qqmusic": "host-delegated"
            }))
            .map_err(|_| ProtocolError::internal());
        }
        let platform = required_platform(request)?;
        serde_json::to_value(self.library_one(platform, request).await?)
            .map_err(|_| ProtocolError::internal())
    }

    async fn library_one(
        &self,
        platform: Platform,
        request: &Request,
    ) -> Result<LibrarySnapshot, ProtocolError> {
        let adapter = self.adapter(platform)?;
        let profile_id = profile_id_or_default(request);
        let session = self.store.session(platform, &profile_id).await;
        adapter
            .library(
                &session,
                request
                    .payload
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(100) as u32,
            )
            .await
            .map_err(map_platform_error)
    }

    async fn lyrics(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let adapter = self.adapter(platform)?;
        let profile_id = profile_id(request)?;
        let track_id = required_string(&request.payload, "trackId")?;
        let session = self.store.session(platform, &profile_id).await;
        serde_json::to_value(
            adapter
                .lyrics(&session, &track_id)
                .await
                .map_err(map_platform_error)?,
        )
        .map_err(|_| ProtocolError::internal())
    }

    async fn playback(&self, request: &Request) -> Result<Value, ProtocolError> {
        let platform = required_platform(request)?;
        let adapter = self.adapter(platform)?;
        let profile_id = profile_id(request)?;
        let track_id = required_string(&request.payload, "trackId")?;
        let quality = request.payload.get("quality").and_then(Value::as_str);
        let session = self.store.session(platform, &profile_id).await;
        let mut source = adapter
            .playback(&session, &track_id, quality)
            .await
            .map_err(map_platform_error)?;
        self.register_media(&mut source).await?;
        serde_json::to_value(source).map_err(|_| ProtocolError::internal())
    }

    async fn register_media(
        &self,
        source: &mut yaqmc_music_providers_protocol::PlaybackSource,
    ) -> Result<(), ProtocolError> {
        let upstream = Url::parse(&source.url).map_err(|_| ProtocolError {
            code: "full-playback-unavailable".to_owned(),
            message: "the provider returned an invalid media URL".to_owned(),
            retryable: false,
        })?;
        let host = upstream.host_str().unwrap_or_default();
        if upstream.scheme() != "https"
            || host.is_empty()
            || upstream.username() != ""
            || upstream.password().is_some()
            || upstream.fragment().is_some()
            || host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host.parse::<std::net::IpAddr>().is_ok()
        {
            return Err(ProtocolError {
                code: "full-playback-unavailable".to_owned(),
                message: "the provider returned an unsafe media URL".to_owned(),
                retryable: false,
            });
        }
        let mut content_length = source.content_length;
        let mut mime_type = match source.mime_type.as_deref() {
            Some(value) => Some(validated_media_mime(value).ok_or_else(|| ProtocolError {
                code: "full-playback-unavailable".to_owned(),
                message: "the media source declared a non-audio content type".to_owned(),
                retryable: false,
            })?),
            None => None,
        };
        if content_length.is_none() || mime_type.is_none() {
            let response = self
                .media_client
                .head(upstream.as_str())
                .send()
                .await
                .map_err(|_| ProtocolError {
                    code: "full-playback-unavailable".to_owned(),
                    message: "the media source could not be inspected".to_owned(),
                    retryable: true,
                })?;
            if !response.status().is_success() {
                return Err(ProtocolError {
                    code: "full-playback-unavailable".to_owned(),
                    message: "the media source rejected inspection".to_owned(),
                    retryable: true,
                });
            }
            if let Some(value) = response.content_length() {
                content_length = Some(value);
            }
            if let Some(value) = response.headers().get(header::CONTENT_TYPE) {
                if let Ok(value) = value.to_str() {
                    mime_type = Some(validated_media_mime(value).ok_or_else(|| ProtocolError {
                        code: "full-playback-unavailable".to_owned(),
                        message: "the media source returned a non-audio content type".to_owned(),
                        retryable: false,
                    })?);
                }
            }
        }
        let content_length = content_length.ok_or_else(|| ProtocolError {
            code: "full-playback-unavailable".to_owned(),
            message: "the media source did not provide a bounded length".to_owned(),
            retryable: false,
        })?;
        if content_length == 0 || content_length > 512 * 1024 * 1024 {
            return Err(ProtocolError {
                code: "full-playback-unavailable".to_owned(),
                message: "the media source length is outside the supported range".to_owned(),
                retryable: false,
            });
        }
        let mime_type = mime_type.ok_or_else(|| ProtocolError {
            code: "full-playback-unavailable".to_owned(),
            message: "the media source did not provide an audio content type".to_owned(),
            retryable: false,
        })?;
        let token = Uuid::new_v4().simple().to_string();
        let expires_at_ms = now_ms().saturating_add(MEDIA_TTL_MS);
        let mut media = self.media.lock().await;
        media.retain(|_, entry| entry.expires_at_ms > now_ms());
        if media.len() >= MAX_MEDIA_ENTRIES {
            if let Some(oldest) = media
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at_ms)
                .map(|(token, _)| token.clone())
            {
                media.remove(&oldest);
            }
        }
        media.insert(
            token.clone(),
            MediaEntry {
                upstream_url: source.url.clone(),
                content_length,
                mime_type: mime_type.clone(),
                expires_at_ms,
            },
        );
        let port = self.config.read().await.listen.port;
        source.url = format!("http://127.0.0.1:{port}/media/{token}");
        source.content_length = Some(content_length);
        source.mime_type = Some(mime_type);
        source.expires_at_ms = Some(expires_at_ms);
        Ok(())
    }

    async fn media_response(
        &self,
        token: &str,
        method: &Method,
        request_headers: &HeaderMap,
    ) -> Result<AxumResponse, (StatusCode, String)> {
        if !is_media_token(token) {
            return Err((
                StatusCode::NOT_FOUND,
                "media source was not found".to_owned(),
            ));
        }
        if method != Method::GET && method != Method::HEAD {
            return Err((
                StatusCode::METHOD_NOT_ALLOWED,
                "media source only supports GET and HEAD".to_owned(),
            ));
        }
        let entry = {
            let mut media = self.media.lock().await;
            let now = now_ms();
            media.retain(|_, item| item.expires_at_ms > now);
            media.get(token).cloned()
        }
        .ok_or((
            StatusCode::NOT_FOUND,
            "media source was not found".to_owned(),
        ))?;
        let range =
            match parse_media_range(request_headers.get(header::RANGE), entry.content_length) {
                Ok(range) => range,
                Err(()) => return Ok(range_not_satisfiable_response(entry.content_length)),
            };
        let expected_length = range
            .map(|range| range.end - range.start + 1)
            .unwrap_or(entry.content_length);
        let mut request = self
            .media_client
            .request(
                if method == Method::HEAD {
                    reqwest::Method::HEAD
                } else {
                    reqwest::Method::GET
                },
                &entry.upstream_url,
            )
            .header(header::ACCEPT_ENCODING, "identity");
        if let Some(range) = range {
            request = request.header(
                header::RANGE,
                format!("bytes={}-{}", range.start, range.end),
            );
        }
        let response = request.send().await.map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "the media source could not be reached".to_owned(),
            )
        })?;
        let status = response.status();
        if status.is_redirection() {
            return Err((
                StatusCode::BAD_GATEWAY,
                "the media source returned an unexpected redirect".to_owned(),
            ));
        }
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(range_not_satisfiable_response(entry.content_length));
        }
        let expected_status = if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        if status != expected_status {
            return Err((
                StatusCode::BAD_GATEWAY,
                "the media source rejected the requested bytes".to_owned(),
            ));
        }
        if let Some(expected_range) = range {
            let content_range = response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok());
            if !content_range_matches(content_range, expected_range, entry.content_length) {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "the media source returned an invalid content range".to_owned(),
                ));
            }
        }
        let upstream_length = match response.headers().get(header::CONTENT_LENGTH) {
            Some(value) => value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or((
                    StatusCode::BAD_GATEWAY,
                    "the media source returned an invalid length".to_owned(),
                ))?,
            None => response.content_length().ok_or((
                StatusCode::BAD_GATEWAY,
                "the media source did not provide a bounded response".to_owned(),
            ))?,
        };
        if upstream_length != expected_length {
            return Err((
                StatusCode::BAD_GATEWAY,
                "the media source returned an unexpected length".to_owned(),
            ));
        }
        let mime_type = match response.headers().get(header::CONTENT_TYPE) {
            Some(value) => value.to_str().ok().and_then(validated_media_mime).ok_or((
                StatusCode::BAD_GATEWAY,
                "the media source returned a non-audio content type".to_owned(),
            ))?,
            None => entry.mime_type,
        };
        let response_status = if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        let mut builder = AxumResponse::builder().status(response_status);
        let response_headers = builder.headers_mut().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "response could not be built".to_owned(),
        ))?;
        response_headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&mime_type).map_err(|_| {
                (
                    StatusCode::BAD_GATEWAY,
                    "the media source returned an invalid content type".to_owned(),
                )
            })?,
        );
        response_headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&expected_length.to_string())
                .expect("generated content-length is valid"),
        );
        response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(range) = range {
            response_headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!(
                    "bytes {}-{}/{}",
                    range.start, range.end, entry.content_length
                ))
                .expect("generated content-range is valid"),
            );
        }
        if method == Method::HEAD {
            return builder.body(Body::empty()).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response could not be built".to_owned(),
                )
            });
        }
        let stream = futures::stream::unfold(
            (Box::pin(response.bytes_stream()), 0_u64, false),
            move |(mut upstream, seen, finished)| async move {
                if finished {
                    return None;
                }
                match upstream.next().await {
                    Some(Ok(chunk)) => {
                        let next = seen.saturating_add(chunk.len() as u64);
                        if next > expected_length {
                            Some((
                                Err(std::io::Error::other(
                                    "media response exceeded its declared length",
                                )),
                                (upstream, seen, true),
                            ))
                        } else {
                            Some((Ok(chunk), (upstream, next, false)))
                        }
                    }
                    Some(Err(_)) => Some((
                        Err(std::io::Error::other("media response could not be read")),
                        (upstream, seen, true),
                    )),
                    None if seen == expected_length => None,
                    None => Some((
                        Err(std::io::Error::other(
                            "media response ended before its declared length",
                        )),
                        (upstream, seen, true),
                    )),
                }
            },
        );
        builder.body(Body::from_stream(stream)).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "response could not be built".to_owned(),
            )
        })
    }

    fn adapter(&self, platform: Platform) -> Result<Arc<dyn PlatformAdapter>, ProtocolError> {
        self.adapters
            .get(&platform)
            .cloned()
            .ok_or_else(|| ProtocolError {
                code: "provider-not-configured".to_owned(),
                message: format!("{platform} is not configured"),
                retryable: false,
            })
    }

    pub async fn serve_http(self, listener: TcpListener) -> Result<(), BackendError> {
        let state = Arc::new(self);
        let app = Router::new()
            .route("/health", get(http_health))
            .route("/v1", post(http_request))
            .route("/media/{token}", get(http_media))
            .with_state(state);
        axum::serve(listener, app)
            .await
            .map_err(|_| BackendError::StateWrite)
    }

    pub async fn serve_stdio(self) -> Result<(), BackendError> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut lines = BufReader::new(stdin).lines();
        let mut stdout = tokio::io::BufWriter::new(stdout);
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|_| BackendError::StateRead)?
        {
            if line.len() > MAX_LINE_BYTES {
                let response = Response {
                    id: String::new(),
                    version: PROTOCOL_VERSION,
                    ok: false,
                    result: None,
                    error: Some(ProtocolError::invalid_request("request line is too large")),
                };
                write_response(&mut stdout, &response).await?;
                continue;
            }
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(request) => self.handle(request).await,
                Err(_) => Response {
                    id: String::new(),
                    version: PROTOCOL_VERSION,
                    ok: false,
                    result: None,
                    error: Some(ProtocolError::invalid_request("request is not valid JSON")),
                },
            };
            write_response(&mut stdout, &response).await?;
        }
        Ok(())
    }
}

async fn http_health(State(backend): State<Arc<Backend>>) -> impl IntoResponse {
    let request = Request {
        id: Uuid::new_v4().to_string(),
        version: PROTOCOL_VERSION,
        mode: UseMode::Isolated,
        platform: None,
        profile_id: None,
        operation: "health".to_owned(),
        payload: Value::Null,
    };
    let response = backend.handle(request).await;
    (StatusCode::OK, Json(response))
}

async fn http_request(
    State(backend): State<Arc<Backend>>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> impl IntoResponse {
    let configured = backend.config.read().await.http_auth_token.clone();
    if let Some(expected) = configured {
        let actual = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if actual != format!("Bearer {expected}") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(Response {
                    id: request.id,
                    version: PROTOCOL_VERSION,
                    ok: false,
                    result: None,
                    error: Some(ProtocolError {
                        code: "unauthorized".to_owned(),
                        message: "backend authorization failed".to_owned(),
                        retryable: false,
                    }),
                }),
            );
        }
    }
    (StatusCode::OK, Json(backend.handle(request).await))
}

async fn http_media(
    State(backend): State<Arc<Backend>>,
    AxumPath(token): AxumPath<String>,
    method: Method,
    headers: HeaderMap,
) -> AxumResponse {
    match backend.media_response(&token, &method, &headers).await {
        Ok(response) => response,
        Err((status, message)) => (status, message).into_response(),
    }
}

async fn write_response(
    stdout: &mut tokio::io::BufWriter<tokio::io::Stdout>,
    response: &Response,
) -> Result<(), BackendError> {
    let text = serde_json::to_string(response).map_err(|_| BackendError::StateWrite)?;
    stdout
        .write_all(text.as_bytes())
        .await
        .map_err(|_| BackendError::StateWrite)?;
    stdout
        .write_all(b"\n")
        .await
        .map_err(|_| BackendError::StateWrite)?;
    stdout.flush().await.map_err(|_| BackendError::StateWrite)
}

fn profile_key(platform: Platform, profile_id: &str) -> String {
    format!("{}:{profile_id}", platform.as_str())
}

fn prune_attempts(attempts: &mut HashMap<String, AuthAttemptState>) {
    let now = now_ms();
    attempts.retain(|_, attempt| match &attempt.outcome {
        AuthAttemptOutcome::Active => {
            attempt
                .preparation
                .expires_at_ms
                .saturating_add(AUTH_ATTEMPT_RETENTION_MS)
                > now
        }
        AuthAttemptOutcome::Completed(_)
        | AuthAttemptOutcome::Expired
        | AuthAttemptOutcome::Cancelled => attempt.retained_until_ms > now,
    });
}

fn is_qr_attempt(attempt: &AuthAttemptState) -> bool {
    attempt.preparation.qr_payload.is_some()
}

fn auth_attempt_status(attempt: &AuthAttemptState, status: &str) -> Value {
    let phase = match status {
        "pending" => "waiting-for-scan",
        "expired" => "expired",
        "cancelled" => "cancelled",
        _ => status,
    };
    let qr_payload = matches!(status, "pending")
        .then(|| attempt.preparation.qr_payload.clone())
        .flatten();
    json!({
        "status": status,
        "phase": phase,
        "attemptId": attempt.preparation.attempt_id,
        "platform": attempt.platform,
        "profileId": attempt.profile_id,
        "authorizationUrl": attempt.preparation.authorization_url,
        "qrPayload": qr_payload,
        "expiresAtMs": attempt.preparation.expires_at_ms
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MediaRange {
    start: u64,
    end: u64,
}

fn parse_media_range(
    value: Option<&HeaderValue>,
    content_length: u64,
) -> Result<Option<MediaRange>, ()> {
    let Some(value) = value else { return Ok(None) };
    let value = value.to_str().map_err(|_| ())?;
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    if content_length == 0 {
        return Err(());
    }
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let start = content_length.saturating_sub(suffix);
        return Ok(Some(MediaRange {
            start,
            end: content_length - 1,
        }));
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= content_length {
        return Err(());
    }
    let end = if end.is_empty() {
        content_length - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(content_length - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some(MediaRange { start, end }))
}

fn content_range_matches(value: Option<&str>, expected: MediaRange, total: u64) -> bool {
    let Some(value) = value.and_then(|value| value.strip_prefix("bytes ")) else {
        return false;
    };
    let Some((range, declared_total)) = value.split_once('/') else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    start.parse::<u64>().ok() == Some(expected.start)
        && end.parse::<u64>().ok() == Some(expected.end)
        && declared_total.parse::<u64>().ok() == Some(total)
}

fn range_not_satisfiable_response(content_length: u64) -> AxumResponse {
    let mut response = (
        StatusCode::RANGE_NOT_SATISFIABLE,
        "requested media range is invalid",
    )
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes */{content_length}"))
            .expect("generated content-range is valid"),
    );
    response
        .headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn is_media_token(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validated_media_mime(value: &str) -> Option<String> {
    let value = value.split(';').next()?.trim().to_ascii_lowercase();
    if value.len() <= 128
        && value.strip_prefix("audio/").is_some_and(|subtype| {
            !subtype.is_empty()
                && subtype
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte))
        })
    {
        Some(value)
    } else {
        None
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn profile_id(request: &Request) -> Result<String, ProtocolError> {
    let profile_id = profile_id_or_default(request);
    if profile_id.len() > 64
        || profile_id.is_empty()
        || !profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(ProtocolError::invalid_request("profileId is invalid"));
    }
    Ok(profile_id)
}

fn profile_id_or_default(request: &Request) -> String {
    request
        .profile_id
        .clone()
        .or_else(|| {
            request
                .payload
                .get("profileId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "default".to_owned())
}

fn required_platform(request: &Request) -> Result<Platform, ProtocolError> {
    request
        .platform
        .or_else(|| {
            request
                .payload
                .get("platform")
                .and_then(Value::as_str)
                .and_then(|value| serde_json::from_value(Value::String(value.to_owned())).ok())
        })
        .ok_or_else(|| ProtocolError::invalid_request("platform is required in isolated mode"))
}

fn required_string(payload: &Value, key: &str) -> Result<String, ProtocolError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 512)
        .map(ToOwned::to_owned)
        .ok_or_else(|| ProtocolError::invalid_request(format!("{key} is required")))
}

fn validate_oauth_state(state: &str) -> Result<(), ProtocolError> {
    if state.is_empty() || state.len() > 512 || state.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ProtocolError::invalid_request("OAuth state is invalid"));
    }
    Ok(())
}

fn validate_attempt_id(attempt_id: &str) -> Result<(), ProtocolError> {
    if attempt_id.is_empty()
        || attempt_id.len() > 128
        || attempt_id.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ProtocolError::invalid_request(
            "authentication attempt id is invalid",
        ));
    }
    Ok(())
}

fn authorization_url_with_state(url: &str, state: &str) -> Result<String, ProtocolError> {
    let mut url = Url::parse(url)
        .map_err(|_| ProtocolError::invalid_request("authorization URL is invalid"))?;
    let mut pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "state")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.push(("state".to_owned(), state.to_owned()));
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url.to_string())
}

fn callback_query_value(callback_url: &str, key: &str) -> Option<String> {
    Url::parse(callback_url).ok().and_then(|url| {
        url.query_pairs()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    })
}

fn should_aggregate(request: &Request) -> bool {
    matches!(request.mode, UseMode::Aggregate)
        || (request.platform.is_none() && request.operation.starts_with("aggregate."))
}

fn session_with_completion(
    session: &ProfileSession,
    completion: &AuthCompletion,
) -> ProfileSession {
    ProfileSession {
        platform: session.platform,
        profile_id: session.profile_id.clone(),
        secret: completion.secret.clone().or_else(|| session.secret.clone()),
        access_token: completion
            .access_token
            .clone()
            .or_else(|| session.access_token.clone()),
        refresh_token: completion
            .refresh_token
            .clone()
            .or_else(|| session.refresh_token.clone()),
        expires_at_ms: completion.expires_at_ms.or(session.expires_at_ms),
    }
}

fn map_platform_error(error: PlatformError) -> ProtocolError {
    match error {
        PlatformError::AuthenticationRequired => ProtocolError {
            code: "authentication-required".to_owned(),
            message: "this operation requires a logged-in profile".to_owned(),
            retryable: false,
        },
        PlatformError::AuthenticationPending => ProtocolError {
            code: "authentication-pending".to_owned(),
            message: "authentication is still pending".to_owned(),
            retryable: true,
        },
        PlatformError::Unsupported => {
            ProtocolError::unsupported("the selected platform does not support this operation")
        }
        PlatformError::FullPlaybackUnavailable => ProtocolError {
            code: "full-playback-unavailable".to_owned(),
            message: "the selected platform did not return a playable full-track source".to_owned(),
            retryable: false,
        },
        PlatformError::Configuration(message) => ProtocolError {
            code: "provider-configuration".to_owned(),
            message,
            retryable: false,
        },
        PlatformError::InvalidResponse => ProtocolError {
            code: "invalid-upstream-response".to_owned(),
            message: "the platform returned an invalid response".to_owned(),
            retryable: true,
        },
        PlatformError::Upstream(message) => ProtocolError {
            code: "upstream-error".to_owned(),
            message,
            retryable: true,
        },
    }
}

pub fn default_config_path() -> PathBuf {
    PathBuf::from(DEFAULT_CONFIG_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use httpmock::MockServer;

    fn request(operation: &str, mode: UseMode, platform: Option<Platform>) -> Request {
        Request {
            id: "test".to_owned(),
            version: PROTOCOL_VERSION,
            mode,
            platform,
            profile_id: Some("default".to_owned()),
            operation: operation.to_owned(),
            payload: json!({"query": "hello"}),
        }
    }

    #[test]
    fn default_config_never_contains_a_client_id() {
        assert!(AppConfig::default().spotify.client_id.is_empty());
    }

    #[test]
    fn aggregate_detection_is_explicit() {
        assert!(should_aggregate(&request(
            "aggregate.search",
            UseMode::Isolated,
            None
        )));
        assert!(should_aggregate(&request(
            "catalog.search",
            UseMode::Aggregate,
            None
        )));
        assert!(!should_aggregate(&request(
            "catalog.search",
            UseMode::Isolated,
            Some(Platform::Spotify)
        )));
    }

    #[test]
    fn profile_ids_are_not_allowed_to_carry_account_data() {
        let mut value = request(
            "account.snapshot",
            UseMode::Isolated,
            Some(Platform::Spotify),
        );
        value.profile_id = Some("user@example.com".to_owned());
        assert!(profile_id(&value).is_err());
        value.operation = "catalog.search".to_owned();
        assert!(profile_id(&value).is_err());
    }

    #[test]
    fn media_ranges_are_single_bounded_ranges() {
        let header = HeaderValue::from_static("bytes=2-5");
        assert_eq!(
            parse_media_range(Some(&header), 11).expect("range"),
            Some(MediaRange { start: 2, end: 5 })
        );
        let header = HeaderValue::from_static("bytes=-4");
        assert_eq!(
            parse_media_range(Some(&header), 11).expect("suffix range"),
            Some(MediaRange { start: 7, end: 10 })
        );
        let header = HeaderValue::from_static("bytes=8-");
        assert_eq!(
            parse_media_range(Some(&header), 11).expect("open range"),
            Some(MediaRange { start: 8, end: 10 })
        );
        assert!(parse_media_range(Some(&HeaderValue::from_static("bytes=2-5,7-8")), 11).is_err());
        assert!(parse_media_range(Some(&HeaderValue::from_static("bytes=11-")), 11).is_err());
    }

    #[tokio::test]
    async fn media_proxy_forwards_range_and_expires_tokens() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/track")
                    .header("range", "bytes=2-5");
                then.status(206)
                    .header("content-type", "audio/mpeg")
                    .header("content-range", "bytes 2-5/11")
                    .header("content-length", "4")
                    .body("cdef");
            })
            .await;
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        })
        .expect("backend");
        let token = "a".repeat(32);
        backend.media.lock().await.insert(
            token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/track", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );
        let headers =
            HeaderMap::from_iter([(header::RANGE, HeaderValue::from_static("bytes=2-5"))]);
        let response = backend
            .media_response(&token, &Method::GET, &headers)
            .await
            .expect("range response");
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok()),
            Some("bytes 2-5/11")
        );
        let body = to_bytes(response.into_body(), 1024).await.expect("body");
        assert_eq!(&body[..], b"cdef");
        mock.assert_async().await;

        backend.media.lock().await.insert(
            token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/track", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_sub(1),
            },
        );
        let error = backend
            .media_response(&token, &Method::GET, &HeaderMap::new())
            .await
            .expect_err("expired token");
        assert_eq!(error.0, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn media_proxy_supports_full_get_and_head_without_fallback_mime() {
        let server = MockServer::start_async().await;
        let get_mock = server
            .mock_async(|when, then| {
                when.method("GET").path("/full");
                then.status(200)
                    .header("content-type", "audio/mpeg")
                    .header("content-length", "11")
                    .body("hello world");
            })
            .await;
        let head_mock = server
            .mock_async(|when, then| {
                when.method("HEAD").path("/full");
                then.status(200)
                    .header("content-type", "audio/mpeg")
                    .header("content-length", "11")
                    .body("hello world");
            })
            .await;
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        })
        .expect("backend");
        let token = "b".repeat(32);
        backend.media.lock().await.insert(
            token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/full", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );

        let response = backend
            .media_response(&token, &Method::GET, &HeaderMap::new())
            .await
            .expect("full get");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_LENGTH),
            Some(&HeaderValue::from_static("11"))
        );
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("audio/mpeg"))
        );
        assert_eq!(
            &to_bytes(response.into_body(), 32).await.expect("full body")[..],
            b"hello world"
        );

        let response = backend
            .media_response(&token, &Method::HEAD, &HeaderMap::new())
            .await
            .expect("head");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            &to_bytes(response.into_body(), 32)
                .await
                .expect("empty head body")[..],
            b""
        );
        get_mock.assert_async().await;
        head_mock.assert_async().await;
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn invalid_ranges_return_a_bounded_416_response() {
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        })
        .expect("backend");
        let token = "c".repeat(32);
        backend.media.lock().await.insert(
            token.clone(),
            MediaEntry {
                upstream_url: "http://127.0.0.1:1/never-called".to_owned(),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );
        let headers =
            HeaderMap::from_iter([(header::RANGE, HeaderValue::from_static("bytes=11-"))]);
        let response = backend
            .media_response(&token, &Method::GET, &headers)
            .await
            .expect("416 response");
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok()),
            Some("bytes */11")
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn media_proxy_rejects_non_audio_and_invalid_upstream_ranges() {
        let server = MockServer::start_async().await;
        let html_mock = server
            .mock_async(|when, then| {
                when.method("GET").path("/html");
                then.status(200)
                    .header("content-type", "text/html")
                    .header("content-length", "4")
                    .body("oops");
            })
            .await;
        let range_mock = server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/bad-range")
                    .header("range", "bytes=2-5");
                then.status(206)
                    .header("content-type", "audio/mpeg")
                    .header("content-range", "bytes 2-6/11")
                    .header("content-length", "4")
                    .body("cdef");
            })
            .await;
        let length_mock = server
            .mock_async(|when, then| {
                when.method("GET")
                    .path("/bad-length")
                    .header("range", "bytes=2-5");
                then.status(206)
                    .header("content-type", "audio/mpeg")
                    .header("content-range", "bytes 2-5/11")
                    .header("content-length", "5")
                    .body("cdefg");
            })
            .await;
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        })
        .expect("backend");
        let html_token = "d".repeat(32);
        backend.media.lock().await.insert(
            html_token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/html", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );
        let error = backend
            .media_response(&html_token, &Method::GET, &HeaderMap::new())
            .await
            .expect_err("non-audio response");
        assert_eq!(error.0, StatusCode::BAD_GATEWAY);

        let range_token = "e".repeat(32);
        backend.media.lock().await.insert(
            range_token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/bad-range", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );
        let headers =
            HeaderMap::from_iter([(header::RANGE, HeaderValue::from_static("bytes=2-5"))]);
        let error = backend
            .media_response(&range_token, &Method::GET, &headers)
            .await
            .expect_err("invalid content range");
        assert_eq!(error.0, StatusCode::BAD_GATEWAY);

        let length_token = "f".repeat(32);
        backend.media.lock().await.insert(
            length_token.clone(),
            MediaEntry {
                upstream_url: format!("http://{}/bad-length", server.address()),
                content_length: 11,
                mime_type: "audio/mpeg".to_owned(),
                expires_at_ms: now_ms().saturating_add(30_000),
            },
        );
        let error = backend
            .media_response(&length_token, &Method::GET, &headers)
            .await
            .expect_err("invalid response length");
        assert_eq!(error.0, StatusCode::BAD_GATEWAY);

        html_mock.assert_async().await;
        range_mock.assert_async().await;
        length_mock.assert_async().await;
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn media_values_fail_closed() {
        assert!(is_media_token(&"a".repeat(32)));
        assert!(!is_media_token("short"));
        assert!(!is_media_token(&"g".repeat(32)));
        assert_eq!(
            validated_media_mime("audio/flac"),
            Some("audio/flac".to_owned())
        );
        assert_eq!(
            validated_media_mime("audio/mpeg; charset=binary"),
            Some("audio/mpeg".to_owned())
        );
        assert!(validated_media_mime("text/html").is_none());
        assert!(AppConfig::default().spotify.client_id.is_empty());
    }

    #[tokio::test]
    async fn oauth_preparation_keeps_the_host_attempt_id_and_state() {
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            spotify: SpotifySettings {
                client_id: "user-client-id".to_owned(),
                ..SpotifySettings::default()
            },
            netease: EndpointSettings::default(),
            kugou: EndpointSettings::default(),
            ..AppConfig::default()
        })
        .expect("backend");
        let response = backend
            .handle(Request {
                id: "oauth-prepare".to_owned(),
                version: PROTOCOL_VERSION,
                mode: UseMode::Isolated,
                platform: Some(Platform::Spotify),
                profile_id: Some("personal".to_owned()),
                operation: "auth.prepare".to_owned(),
                payload: json!({
                    "method": "browser-oauth",
                    "attemptId": "oauth-host-attempt",
                    "state": "host-state"
                }),
            })
            .await;
        assert!(response.ok);
        let result = response.result.expect("OAuth preparation");
        assert_eq!(result["attemptId"], "oauth-host-attempt");
        assert!(result["authorizationUrl"]
            .as_str()
            .expect("authorization URL")
            .contains("state=host-state"));
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn qr_pending_cancel_and_refresh_are_idempotent() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/login/qr/key");
                then.status(200).json_body(json!({
                    "code": 200,
                    "data": {"unikey": "qr-key"}
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/login/qr/create");
                then.status(200).json_body(json!({
                    "code": 200,
                    "data": {
                        "qrurl": "https://music.163.com/qr/qr-key",
                        "qrimg": "data:image/png;base64,AA=="
                    }
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/login/qr/check");
                then.status(200).json_body(json!({"code": 801}));
            })
            .await;
        let data_dir = std::env::temp_dir().join(format!(
            "yaqmc-music-providers-test-{}",
            Uuid::new_v4().simple()
        ));
        let backend = Backend::new(AppConfig {
            data_dir: data_dir.clone(),
            netease: EndpointSettings {
                base_url: format!("http://{}", server.address()),
            },
            kugou: EndpointSettings::default(),
            ..AppConfig::default()
        })
        .expect("backend");
        let base = |operation: &str, payload: Value| Request {
            id: operation.to_owned(),
            version: PROTOCOL_VERSION,
            mode: UseMode::Isolated,
            platform: Some(Platform::Netease),
            profile_id: Some("work".to_owned()),
            operation: operation.to_owned(),
            payload,
        };
        let started = backend
            .handle(base("auth.prepare", json!({"method": "qr"})))
            .await;
        assert!(started.ok);
        let attempt_id = started.result.as_ref().unwrap()["attemptId"]
            .as_str()
            .unwrap()
            .to_owned();

        let pending = backend
            .handle(base(
                "auth.complete",
                json!({"attemptId": attempt_id, "flow": "qr"}),
            ))
            .await;
        assert!(pending.ok);
        assert_eq!(pending.result.unwrap()["status"], "pending");

        let cancelled = backend
            .handle(base(
                "auth.cancel",
                json!({"attemptId": attempt_id, "flow": "qr"}),
            ))
            .await;
        assert!(cancelled.ok);
        assert_eq!(cancelled.result.unwrap()["status"], "cancelled");

        let refreshed = backend
            .handle(base(
                "auth.refresh",
                json!({"attemptId": attempt_id, "flow": "qr"}),
            ))
            .await;
        assert!(refreshed.ok);
        assert_ne!(
            refreshed.result.unwrap()["attemptId"],
            Value::String(attempt_id)
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
