use crate::{
    bounded_limit,
    http::{bearer, ApiHttp},
    now_ms, require_non_empty, AuthCompletion, AuthPreparation, PlatformAdapter, PlatformError,
    ProfileSession,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rng, Rng};
use reqwest::{redirect::Policy, Method};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::form_urlencoded;
use uuid::Uuid;
use yaqmc_music_providers_protocol::{
    AccountSnapshot, AuthMethod, LibrarySnapshot, Lyrics, Platform, PlaybackSource,
    PlaylistSummary, SearchResult, Track, TrackSource,
};

const DEFAULT_ACCOUNTS_BASE: &str = "https://accounts.spotify.com";
const DEFAULT_API_BASE: &str = "https://api.spotify.com";
const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:43821/oauth/spotify/callback";

#[derive(Clone, Debug)]
pub struct SpotifyConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub accounts_base_url: String,
    pub api_base_url: String,
}

impl SpotifyConfig {
    pub fn from_client_id(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            redirect_uri: DEFAULT_REDIRECT_URI.to_owned(),
            scopes: vec![
                "user-read-private".to_owned(),
                "user-read-email".to_owned(),
                "user-library-read".to_owned(),
                "playlist-read-private".to_owned(),
                "playlist-read-collaborative".to_owned(),
                "streaming".to_owned(),
            ],
            accounts_base_url: DEFAULT_ACCOUNTS_BASE.to_owned(),
            api_base_url: DEFAULT_API_BASE.to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct SpotifyClient {
    config: SpotifyConfig,
    api: ApiHttp,
    client: reqwest::Client,
}

impl SpotifyClient {
    pub fn new(config: SpotifyConfig) -> Result<Self, PlatformError> {
        require_non_empty(&config.client_id, "Spotify client ID")?;
        let api = ApiHttp::new(&config.api_base_url, false)?;
        ApiHttp::validate_base_url(&config.accounts_base_url, false)?;
        let client = reqwest::Client::builder()
            .user_agent("yaqmc-music-providers/0.1")
            .timeout(std::time::Duration::from_secs(20))
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                PlatformError::Configuration("Spotify HTTP client could not start".to_owned())
            })?;
        Ok(Self {
            config,
            api,
            client,
        })
    }

    pub fn authorization_url(&self, state: &str, verifier: &str) -> Result<String, PlatformError> {
        require_non_empty(state, "OAuth state")?;
        validate_verifier(verifier)?;
        let challenge = pkce_challenge(verifier);
        let scope = self.config.scopes.join(" ");
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("client_id", &self.config.client_id);
        query.append_pair("response_type", "code");
        query.append_pair("redirect_uri", &self.config.redirect_uri);
        query.append_pair("state", state);
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("code_challenge", &challenge);
        query.append_pair("scope", &scope);
        Ok(format!(
            "{}/authorize?{}",
            self.config.accounts_base_url.trim_end_matches('/'),
            query.finish()
        ))
    }

    pub fn random_auth_material(&self) -> (String, String) {
        let state = Uuid::new_v4().to_string();
        let mut bytes = [0_u8; 48];
        rng().fill(&mut bytes);
        (state, URL_SAFE_NO_PAD.encode(bytes))
    }

    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
    ) -> Result<AuthCompletion, PlatformError> {
        require_non_empty(code, "authorization code")?;
        validate_verifier(verifier)?;
        let body = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("client_id", self.config.client_id.as_str()),
            ("code_verifier", verifier),
        ];
        let response = self
            .client
            .post(format!(
                "{}/api/token",
                self.config.accounts_base_url.trim_end_matches('/')
            ))
            .form(&body)
            .send()
            .await
            .map_err(|error| PlatformError::Upstream(safe_error(&error)))?;
        if !response.status().is_success() {
            return Err(PlatformError::Upstream(format!(
                "Spotify token endpoint returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let token: TokenResponse = response
            .json()
            .await
            .map_err(|_| PlatformError::InvalidResponse)?;
        Ok(AuthCompletion {
            access_token: Some(token.access_token),
            refresh_token: token.refresh_token,
            expires_at_ms: Some(now_ms().saturating_add(u64::from(token.expires_in) * 1000)),
            ..AuthCompletion::default()
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<AuthCompletion, PlatformError> {
        require_non_empty(refresh_token, "Spotify refresh token")?;
        let body = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.config.client_id.as_str()),
        ];
        let response = self
            .client
            .post(format!(
                "{}/api/token",
                self.config.accounts_base_url.trim_end_matches('/')
            ))
            .form(&body)
            .send()
            .await
            .map_err(|error| PlatformError::Upstream(safe_error(&error)))?;
        if !response.status().is_success() {
            return Err(PlatformError::AuthenticationRequired);
        }
        let token: TokenResponse = response
            .json()
            .await
            .map_err(|_| PlatformError::InvalidResponse)?;
        Ok(AuthCompletion {
            access_token: Some(token.access_token),
            refresh_token: token
                .refresh_token
                .or_else(|| Some(refresh_token.to_owned())),
            expires_at_ms: Some(now_ms().saturating_add(u64::from(token.expires_in) * 1000)),
            ..AuthCompletion::default()
        })
    }

    async fn ensure_token(&self, session: &ProfileSession) -> Result<String, PlatformError> {
        if let Some(token) = &session.access_token {
            if session
                .expires_at_ms
                .is_none_or(|expires| expires > now_ms().saturating_add(30_000))
            {
                return Ok(token.clone());
            }
        }
        let refresh = session
            .refresh_token
            .as_deref()
            .ok_or(PlatformError::AuthenticationRequired)?;
        Ok(self
            .refresh_token(refresh)
            .await?
            .access_token
            .expect("refresh returns access token"))
    }

    async fn api_json<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        method: Method,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, PlatformError> {
        self.api
            .json(method, path, query, &[bearer(token)], None)
            .await
    }
}

#[async_trait]
impl PlatformAdapter for SpotifyClient {
    fn platform(&self) -> Platform {
        Platform::Spotify
    }

    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod {
            id: "browser-oauth".to_owned(),
            label: "Spotify browser login (PKCE)".to_owned(),
            requires_login: false,
        }]
    }

    async fn prepare_auth(
        &self,
        _session: &ProfileSession,
        method: &str,
    ) -> Result<AuthPreparation, PlatformError> {
        if method != "browser-oauth" {
            return Err(PlatformError::Unsupported);
        }
        let (state, verifier) = self.random_auth_material();
        Ok(AuthPreparation {
            attempt_id: Uuid::new_v4().to_string(),
            authorization_url: Some(self.authorization_url(&state, &verifier)?),
            qr_payload: None,
            expires_at_ms: now_ms().saturating_add(10 * 60 * 1000),
            verifier: Some(verifier),
            state: Some(state),
        })
    }

    async fn complete_auth(
        &self,
        _session: &ProfileSession,
        preparation: &AuthPreparation,
        payload: &Value,
    ) -> Result<AuthCompletion, PlatformError> {
        let code = payload
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| PlatformError::Configuration("OAuth code is required".to_owned()))?;
        let verifier = preparation
            .verifier
            .as_deref()
            .ok_or(PlatformError::InvalidResponse)?;
        self.exchange_code(code, verifier).await
    }

    async fn account_snapshot(
        &self,
        session: &ProfileSession,
    ) -> Result<AccountSnapshot, PlatformError> {
        let Some(token) = session.access_token.as_deref() else {
            return Ok(AccountSnapshot {
                platform: Platform::Spotify,
                profile_id: session.profile_id.clone(),
                authenticated: false,
                display_name: None,
                masked_identity: None,
                entitlement: None,
            });
        };
        let me: CurrentUser = self.api_json(token, Method::GET, "/v1/me", &[]).await?;
        Ok(AccountSnapshot {
            platform: Platform::Spotify,
            profile_id: session.profile_id.clone(),
            authenticated: true,
            display_name: me.display_name,
            masked_identity: me.email.map(|email| mask_identity(&email)),
            entitlement: me.product,
        })
    }

    async fn search(
        &self,
        session: &ProfileSession,
        query: &str,
        page: u32,
        limit: u32,
    ) -> Result<SearchResult, PlatformError> {
        let token = self.ensure_token(session).await?;
        let limit = bounded_limit(limit);
        let offset = page.saturating_mul(limit);
        let response: SearchResponse = self
            .api_json(
                &token,
                Method::GET,
                "/v1/search",
                &[
                    ("q", query.to_owned()),
                    ("type", "track".to_owned()),
                    ("limit", limit.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;
        Ok(SearchResult {
            items: response
                .tracks
                .items
                .into_iter()
                .map(|track| map_track(track, &session.profile_id))
                .collect(),
            next_cursor: if response.tracks.next.is_some() {
                Some((page + 1).to_string())
            } else {
                None
            },
            platform: Some(Platform::Spotify),
            warnings: Vec::new(),
        })
    }

    async fn library(
        &self,
        session: &ProfileSession,
        limit: u32,
    ) -> Result<LibrarySnapshot, PlatformError> {
        let token = self.ensure_token(session).await?;
        let limit = bounded_limit(limit);
        let saved: SavedTracks = self
            .api_json(
                &token,
                Method::GET,
                "/v1/me/tracks",
                &[("limit", limit.to_string())],
            )
            .await?;
        let playlists: PlaylistPage = self
            .api_json(
                &token,
                Method::GET,
                "/v1/me/playlists",
                &[("limit", limit.to_string())],
            )
            .await?;
        Ok(LibrarySnapshot {
            platform: Platform::Spotify,
            profile_id: session.profile_id.clone(),
            authenticated: true,
            tracks: saved
                .items
                .into_iter()
                .map(|item| map_track(item.track, &session.profile_id))
                .collect(),
            playlists: playlists
                .items
                .into_iter()
                .map(|playlist| PlaylistSummary {
                    id: playlist.id,
                    title: playlist.name,
                    track_count: playlist.tracks.total,
                    artwork_url: playlist.images.first().and_then(|image| image.url.clone()),
                    source: TrackSource {
                        platform: Platform::Spotify,
                        profile_id: session.profile_id.clone(),
                        track_id: String::new(),
                    },
                })
                .collect(),
            warnings: Vec::new(),
        })
    }

    async fn lyrics(
        &self,
        _session: &ProfileSession,
        _track_id: &str,
    ) -> Result<Lyrics, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    async fn playback(
        &self,
        session: &ProfileSession,
        track_id: &str,
        _quality: Option<&str>,
    ) -> Result<PlaybackSource, PlatformError> {
        let token = self.ensure_token(session).await?;
        let track: SpotifyTrack = self
            .api_json(&token, Method::GET, &format!("/v1/tracks/{track_id}"), &[])
            .await?;
        let url = track
            .preview_url
            .ok_or(PlatformError::FullPlaybackUnavailable)?;
        Ok(PlaybackSource {
            source: TrackSource {
                platform: Platform::Spotify,
                profile_id: session.profile_id.clone(),
                track_id: track.id,
            },
            url,
            mime_type: Some("audio/mpeg".to_owned()),
            content_length: None,
            expires_at_ms: None,
            is_preview: true,
            quality: Some("preview".to_owned()),
        })
    }
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn validate_verifier(verifier: &str) -> Result<(), PlatformError> {
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
    {
        return Err(PlatformError::Configuration(
            "OAuth verifier is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn map_track(track: SpotifyTrack, profile_id: &str) -> Track {
    Track {
        source: TrackSource {
            platform: Platform::Spotify,
            profile_id: profile_id.to_owned(),
            track_id: track.id.clone(),
        },
        title: track.name,
        artists: track
            .artists
            .into_iter()
            .map(|artist| artist.name)
            .collect(),
        album: Some(track.album.name),
        duration_ms: Some(track.duration_ms),
        artwork_url: track
            .album
            .images
            .first()
            .and_then(|image| image.url.clone()),
        preview_url: track.preview_url,
        playable: true,
        metadata: json!({"externalUrl": track.external_urls.spotify}),
    }
}

fn mask_identity(value: &str) -> String {
    let mut chars = value.chars();
    let first = chars.next().unwrap_or('*');
    let last = value.chars().last().unwrap_or('*');
    format!("{first}***{last}")
}

fn safe_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".to_owned()
    } else if error.is_connect() {
        "connection failed".to_owned()
    } else {
        "request failed".to_owned()
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u32,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CurrentUser {
    display_name: Option<String>,
    email: Option<String>,
    product: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    tracks: TrackPage,
}

#[derive(Debug, Deserialize)]
struct TrackPage {
    items: Vec<SpotifyTrack>,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SavedTracks {
    items: Vec<SavedTrack>,
}

#[derive(Debug, Deserialize)]
struct SavedTrack {
    track: SpotifyTrack,
}

#[derive(Debug, Deserialize)]
struct PlaylistPage {
    items: Vec<SpotifyPlaylist>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylist {
    id: String,
    name: String,
    tracks: PlaylistTracks,
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Deserialize)]
struct PlaylistTracks {
    total: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrack {
    id: String,
    name: String,
    duration_ms: u64,
    preview_url: Option<String>,
    album: SpotifyAlbum,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
    #[serde(default)]
    external_urls: ExternalUrls,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbum {
    name: String,
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct ExternalUrls {
    spotify: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyImage {
    url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_rfc7636_s256() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorization_url_contains_user_client_id_and_no_secret() {
        let client =
            SpotifyClient::new(SpotifyConfig::from_client_id("user-client-id")).expect("client");
        let url = client
            .authorization_url("state-value", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            .expect("url");
        assert!(url.contains("client_id=user-client-id"));
        assert!(!url.contains("client_secret"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn authorization_url_uses_the_configured_accounts_origin() {
        let mut config = SpotifyConfig::from_client_id("user-client-id");
        config.accounts_base_url = "https://accounts.example.test/api".to_owned();
        let client = SpotifyClient::new(config).expect("client");
        let url = client
            .authorization_url("state-value", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            .expect("url");
        assert!(url.starts_with("https://accounts.example.test/api/authorize?"));
    }

    #[test]
    fn invalid_client_id_is_rejected_before_network() {
        assert!(matches!(
            SpotifyClient::new(SpotifyConfig::from_client_id("")),
            Err(PlatformError::Configuration(_))
        ));
    }
}
