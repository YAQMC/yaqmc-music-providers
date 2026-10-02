use super::PlatformError;
use reqwest::{header, redirect::Policy, Client, Method, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::net::IpAddr;

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct ApiHttp {
    client: Client,
    base: Url,
}

impl ApiHttp {
    pub fn new(base: &str, allow_http_loopback: bool) -> Result<Self, PlatformError> {
        let base = validate_base_url(base, allow_http_loopback)?;
        let client = Client::builder()
            .user_agent("yaqmc-music-providers/0.1")
            .timeout(std::time::Duration::from_secs(20))
            .redirect(Policy::none())
            .build()
            .map_err(|_| PlatformError::Configuration("HTTP client could not start".to_owned()))?;
        Ok(Self { client, base })
    }

    pub(crate) fn validate_base_url(
        base: &str,
        allow_http_loopback: bool,
    ) -> Result<Url, PlatformError> {
        validate_base_url(base, allow_http_loopback)
    }

    pub async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        headers: &[(header::HeaderName, String)],
        body: Option<Value>,
    ) -> Result<T, PlatformError> {
        let url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| PlatformError::Configuration("API path is invalid".to_owned()))?;
        let mut request = self.client.request(method, url).query(query);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| PlatformError::Upstream(safe_reqwest_error(&error)))?;
        let status = response.status();
        if status.is_redirection() {
            return Err(PlatformError::Upstream(
                "upstream redirects are not followed".to_owned(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| PlatformError::Upstream("response body could not be read".to_owned()))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(PlatformError::Upstream(
                "response body is too large".to_owned(),
            ));
        }
        if !status.is_success() {
            return Err(PlatformError::Upstream(format!(
                "upstream returned HTTP {}",
                status.as_u16()
            )));
        }
        serde_json::from_slice(&bytes).map_err(|_| PlatformError::InvalidResponse)
    }
}

fn validate_base_url(base: &str, allow_http_loopback: bool) -> Result<Url, PlatformError> {
    let base = Url::parse(base)
        .map_err(|_| PlatformError::Configuration("API base URL is invalid".to_owned()))?;
    let host = base
        .host_str()
        .ok_or_else(|| PlatformError::Configuration("API base URL has no host".to_owned()))?;
    if !base.username().is_empty() || base.password().is_some() {
        return Err(PlatformError::Configuration(
            "API base URL must not contain credentials".to_owned(),
        ));
    }
    if base.query().is_some() || base.fragment().is_some() {
        return Err(PlatformError::Configuration(
            "API base URL must not contain query or fragment".to_owned(),
        ));
    }
    let loopback = host
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or(matches!(host, "localhost"));
    if base.scheme() != "https" && !(allow_http_loopback && loopback) {
        return Err(PlatformError::Configuration(
            "API base URL must use HTTPS; HTTP is allowed only for loopback fixtures".to_owned(),
        ));
    }
    Ok(base)
}

pub(crate) fn bearer(token: &str) -> (header::HeaderName, String) {
    (header::AUTHORIZATION, format!("Bearer {token}"))
}

pub(crate) fn cookie(cookie: &str) -> (header::HeaderName, String) {
    (header::COOKIE, cookie.to_owned())
}

fn safe_reqwest_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".to_owned()
    } else if error.is_connect() {
        "connection failed".to_owned()
    } else {
        "request failed".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_validation_rejects_credentials_and_non_loopback_http() {
        assert!(validate_base_url("https://music.example", false).is_ok());
        assert!(validate_base_url("https://user:pass@music.example", false).is_err());
        assert!(validate_base_url("http://10.0.0.1:8080", true).is_err());
        assert!(validate_base_url("http://127.0.0.1:8080", true).is_ok());
        assert!(validate_base_url("https://music.example/api?token=x", false).is_err());
    }
}
