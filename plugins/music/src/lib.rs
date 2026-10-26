#![no_std]

extern crate alloc;

mod netease_catalog;
mod netease_comments;

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyInit};
use aes::Aes128;
use alloc::{
    alloc::{alloc, dealloc, realloc, Layout},
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ecb::Encryptor;
use md5::{Digest as Md5Digest, Md5};
use num_bigint::BigUint;
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{
    de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor},
    Deserialize,
};
use serde_json::{json, Map, Value};
use sha2::Sha256;

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[unsafe(no_mangle)]
unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, length: usize) -> i32 {
    for index in 0..length {
        let left = left.add(index).read();
        let right = right.add(index).read();
        if left != right {
            return i32::from(left) - i32::from(right);
        }
    }
    0
}

#[unsafe(export_name = "cabi_realloc")]
unsafe extern "C" fn canonical_realloc(
    old_ptr: *mut u8,
    old_len: usize,
    align: usize,
    new_len: usize,
) -> *mut u8 {
    if old_len == 0 {
        if new_len == 0 {
            return align as *mut u8;
        }
        let layout = Layout::from_size_align_unchecked(new_len, align);
        let ptr = alloc(layout);
        if ptr.is_null() {
            core::hint::unreachable_unchecked();
        }
        return ptr;
    }
    let layout = Layout::from_size_align_unchecked(old_len, align);
    if new_len == 0 {
        dealloc(old_ptr, layout);
        return align as *mut u8;
    }
    let ptr = realloc(old_ptr, layout, new_len);
    if ptr.is_null() {
        core::hint::unreachable_unchecked();
    }
    ptr
}

wit_bindgen::generate!({
    path: "../../wit/yaqmc-provider",
    world: "provider-account",
});

const PLUGIN_ID: &str = "org.yaqmc.providers.music";
const DEFAULT_PROFILE: &str = "default";
const SPOTIFY_ACCOUNTS: &str = "https://accounts.spotify.com";
const SPOTIFY_API: &str = "https://api.spotify.com";
const NETEASE_API: &str = "https://interfacepc.music.163.com";
const KUGOU_API: &str = "https://gateway.kugou.com";
const KUGOU_LOGIN: &str = "https://login-user.kugou.com";
const KUGOU_LYRICS: &str = "https://lyrics.kugou.com";
const KUGOU_MEDIA: &str = "https://trackercdn.kugou.com";
const KUGOU_MOBILE: &str = "https://m.kugou.com";
const SPOTIFY_CALLBACK: &str = "http://127.0.0.1:43821/oauth/spotify/callback";

struct MusicProvider;

impl Guest for MusicProvider {
    fn invoke(
        capability: String,
        operation: String,
        payload_json: String,
    ) -> Result<String, String> {
        let payload = parse_object(&payload_json)?;
        let platform = payload
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(platform, "spotify" | "netease" | "kugou") {
            return Err(error(
                "invalid-request",
                "the provider platform is missing or unsupported",
                false,
            ));
        }
        dispatch(&capability, &operation, platform, &payload)
    }
}

export!(MusicProvider);

fn parse_object(payload: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| error("invalid-request", "payload is not valid JSON", false))?;
    if value.is_object() {
        Ok(value)
    } else {
        Ok(Value::Object(Map::new()))
    }
}

fn error(code: &str, message: &str, retryable: bool) -> String {
    json!({"code": code, "message": message, "retryable": retryable}).to_string()
}

fn unsupported(operation: &str) -> String {
    error(
        "unsupported-operation",
        &format!("the music provider does not support {operation}"),
        false,
    )
}

fn platform_id(payload: &Value) -> &str {
    payload
        .get("profileId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROFILE)
}

fn now_ms() -> u64 {
    yaqmc::provider::utilities::unix_millis()
}

fn random_bytes(length: u32) -> Result<Vec<u8>, String> {
    yaqmc::provider::utilities::random_bytes(length)
}

fn random_token(prefix: &str) -> Result<String, String> {
    let bytes = random_bytes(24)?;
    let mut value = String::with_capacity(prefix.len() + bytes.len() * 2);
    value.push_str(prefix);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    Ok(value)
}

fn setting(name: &str) -> Option<String> {
    let raw = yaqmc::provider::settings::get().ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn storage_get(key: &str) -> Result<Option<String>, String> {
    yaqmc::provider::storage::kv_get(key)
}

fn storage_set(key: &str, value: &str) -> Result<(), String> {
    yaqmc::provider::storage::kv_set(key, value)
        .map(|_| ())
        .map_err(|_| error("storage-failure", "provider state could not be saved", true))
}

fn storage_delete(key: &str) -> Result<(), String> {
    yaqmc::provider::storage::kv_delete(key)
        .map(|_| ())
        .map_err(|_| {
            error(
                "storage-failure",
                "provider state could not be deleted",
                true,
            )
        })
}

fn credential_handle(key: &str) -> Result<Option<String>, String> {
    storage_get(key)
}

fn replace_credential(key: &str, origin: &str, secret: &str) -> Result<String, String> {
    let previous = credential_handle(key)?;
    let handle = yaqmc::provider::credentials::create(origin, secret).map_err(|_| {
        error(
            "storage-failure",
            "provider credentials could not be saved",
            true,
        )
    })?;
    if let Err(reason) = storage_set(key, &handle) {
        let _ = yaqmc::provider::credentials::delete(&handle);
        return Err(reason);
    }
    if let Some(previous) = previous {
        let _ = yaqmc::provider::credentials::delete(&previous);
    }
    Ok(handle)
}

fn clear_credential(key: &str) -> Result<(), String> {
    if let Some(handle) = credential_handle(key)? {
        let _ = yaqmc::provider::credentials::delete(&handle);
    }
    storage_delete(key)
}

fn request(
    method: &str,
    url: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    credential: Option<(&str, &str, &str)>,
    credential_body: Option<(&str, &str)>,
) -> Result<Value, String> {
    Ok(request_with_meta(method, url, body, content_type, credential, credential_body)?.body)
}

struct JsonResponse<Body = Value> {
    body: Body,
    set_cookies: Vec<String>,
    content_length: Option<u64>,
    mime_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkResponse<Body> {
    status: u16,
    #[serde(default)]
    body: String,
    body_json: Option<Body>,
    #[serde(default)]
    set_cookies: Vec<String>,
    content_length: Option<u64>,
    mime_type: Option<String>,
}

fn request_with_meta(
    method: &str,
    url: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    credential: Option<(&str, &str, &str)>,
    credential_body: Option<(&str, &str)>,
) -> Result<JsonResponse, String> {
    request_with_meta_headers(
        method,
        url,
        body,
        content_type,
        credential,
        credential_body,
        &[],
    )
}

fn request_with_meta_headers(
    method: &str,
    url: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    credential: Option<(&str, &str, &str)>,
    credential_body: Option<(&str, &str)>,
    extra_headers: &[(&str, String)],
) -> Result<JsonResponse, String> {
    request_with_meta_headers_as(
        method,
        url,
        body,
        content_type,
        credential,
        credential_body,
        extra_headers,
    )
}

fn request_with_meta_headers_as<Body: DeserializeOwned>(
    method: &str,
    url: &str,
    body: Option<&str>,
    content_type: Option<&str>,
    credential: Option<(&str, &str, &str)>,
    credential_body: Option<(&str, &str)>,
    extra_headers: &[(&str, String)],
) -> Result<JsonResponse<Body>, String> {
    let mut headers = Map::new();
    headers.insert(
        "accept".to_owned(),
        Value::String("application/json, text/plain, */*".to_owned()),
    );
    headers.insert(
        "user-agent".to_owned(),
        Value::String("YAQMC/1.0 music-provider".to_owned()),
    );
    if let Some(content_type) = content_type {
        headers.insert(
            "content-type".to_owned(),
            Value::String(content_type.to_owned()),
        );
    }
    for (name, value) in extra_headers {
        headers.insert((*name).to_owned(), Value::String(value.clone()));
    }
    let mut request = json!({
        "method": method,
        "url": url,
        "headers": headers,
        "responseFormat": "json",
    });
    if let Some(body) = body {
        request["body"] = Value::String(body.to_owned());
    }
    if let Some((handle, name, prefix)) = credential {
        request["credentialHeaders"] = json!([{
            "handle": handle,
            "name": name,
            "prefix": prefix,
        }]);
        if name == "cookie"
            && (url.starts_with("https://gateway.kugou.com/")
                || url.starts_with("https://trackercdn.kugou.com/"))
        {
            let scheme = if method == "POST"
                && url.split('?').next() == Some("https://gateway.kugou.com/v3/get_my_info")
            {
                "kugou-user-info"
            } else if method == "POST"
                && matches!(
                    url.split('?').next(),
                    Some("https://gateway.kugou.com/v7/get_all_list")
                        | Some("https://gateway.kugou.com/v4/get_list_all_file_v3")
                )
            {
                "kugou-cloud-list"
            } else {
                "kugou-android"
            };
            request["credentialQuerySigning"] = json!({
                "scheme": scheme,
                "handle": handle,
            });
        }
    }
    if let Some((handle, name)) = credential_body {
        request["credentialBodyFields"] = json!([{
            "handle": handle,
            "name": name,
        }]);
    }
    let raw = yaqmc::provider::network::request(&request.to_string())
        .map_err(|_| error("offline", "the upstream music service is unavailable", true))?;
    let response: NetworkResponse<Body> = serde_json::from_str(&raw).map_err(|_| {
        error(
            "malformed-response",
            "YAQMC returned an invalid network response",
            true,
        )
    })?;
    let status = u64::from(response.status);
    let body = response.body.as_str();
    if !(200..300).contains(&status) {
        let code = upstream_error_code(url, status, body);
        return Err(error(
            code,
            "the upstream music service rejected the request",
            status == 429 || status >= 500,
        ));
    }
    let body = match response.body_json {
        Some(body) => body,
        None => serde_json::from_str(if body.trim().is_empty() { "null" } else { body }).map_err(
            |_| {
                error(
                    "malformed-response",
                    "the upstream response is not valid JSON",
                    true,
                )
            },
        )?,
    };
    Ok(JsonResponse {
        body,
        set_cookies: response.set_cookies,
        content_length: response.content_length,
        mime_type: response.mime_type,
    })
}

fn upstream_error_code(url: &str, status: u64, body: &str) -> &'static str {
    if url.starts_with("https://api.spotify.com/") && status == 403 {
        return "authorization-rejected";
    }
    if url == "https://accounts.spotify.com/api/token" {
        if let Ok(response) = serde_json::from_str::<Value>(body) {
            match response.get("error").and_then(Value::as_str) {
                Some("invalid_grant") => return "authentication-expired",
                Some("invalid_client" | "unauthorized_client") => return "provider-not-configured",
                Some("access_denied") => return "authorization-rejected",
                _ => {}
            }
        }
    }
    match status {
        401 | 403 => "authentication-expired",
        429 => "rate-limited",
        _ => "offline",
    }
}

fn netease_request(profile: &str, path: &str, payload: Value) -> Result<Value, String> {
    let response = netease_response::<Value>(profile, path, payload)?;
    let mut body = response.body;
    if body.get("cookie").is_none() {
        if let Some(cookie) = cookies_from_list(&response.set_cookies) {
            if let Some(object) = body.as_object_mut() {
                object.insert("cookie".to_owned(), Value::String(cookie));
            }
        }
    }
    Ok(body)
}

fn netease_response<Body: DeserializeOwned>(
    profile: &str,
    path: &str,
    payload: Value,
) -> Result<JsonResponse<Body>, String> {
    let encrypted = netease_encrypted_params(path, &payload);
    let credential = credential_handle(&credential_key("netease", profile, "cookie"))?;
    request_with_meta_headers_as(
        "POST",
        &netease_eapi_url(path)?,
        Some(&format!("params={encrypted}")),
        Some("application/x-www-form-urlencoded"),
        credential
            .as_ref()
            .map(|value| (value.as_str(), "cookie", "")),
        None,
        &[],
    )
}

fn netease_eapi_url(path: &str) -> Result<String, String> {
    let suffix = path
        .strip_prefix("/api/")
        .ok_or_else(|| error("invalid-request", "NetEase API path is invalid", false))?;
    Ok(format!("{NETEASE_API}/eapi/{suffix}"))
}

fn netease_encrypted_params(path: &str, payload: &Value) -> String {
    let text = payload.to_string();
    let digest = md5_hex(&format!("nobody{path}use{text}md5forencrypt")).to_ascii_lowercase();
    let plaintext = format!("{path}-36cd479b6b5-{text}-36cd479b6b5-{digest}");
    aes_ecb_pkcs7_hex(plaintext.as_bytes(), b"e82ckenh8dichen8")
}

fn require_netease_success(response: &Value) -> Result<(), String> {
    match response.get("code").and_then(Value::as_i64) {
        Some(200) => Ok(()),
        Some(301 | 401) => Err(error(
            "authentication-expired",
            "NetEase account authorization is required",
            false,
        )),
        Some(429) => Err(error(
            "rate-limited",
            "NetEase rate limit was reached",
            true,
        )),
        Some(_) => Err(error(
            "provider-unavailable",
            "NetEase rejected the request",
            true,
        )),
        None => Err(error(
            "malformed-response",
            "NetEase returned no response code",
            true,
        )),
    }
}

fn aes_ecb_pkcs7_hex(data: &[u8], key: &[u8; 16]) -> String {
    let encrypted = Encryptor::<Aes128>::new(key.into()).encrypt_padded_vec_mut::<Pkcs7>(data);
    let mut output = String::with_capacity(encrypted.len() * 2);
    for byte in encrypted {
        output.push(hex_upper(byte >> 4));
        output.push(hex_upper(byte & 0x0f));
    }
    output
}

fn query_url(base: &str, path: &str, params: &[(&str, String)]) -> String {
    let mut url = String::from(base.trim_end_matches('/'));
    url.push('/');
    url.push_str(path.trim_start_matches('/'));
    if !params.is_empty() {
        url.push('?');
        for (index, (key, value)) in params.iter().enumerate() {
            if index != 0 {
                url.push('&');
            }
            url.push_str(&form_encode(key));
            url.push('=');
            url.push_str(&form_encode(value));
        }
    }
    url
}

fn media_metadata(url: &str) -> Result<(u64, String), String> {
    let response = request_with_meta("HEAD", url, None, None, None, None)?;
    let length = response.content_length.ok_or_else(|| {
        error(
            "full-playback-unavailable",
            "the media source did not provide a content length",
            false,
        )
    })?;
    if length == 0 {
        return Err(error(
            "full-playback-unavailable",
            "the media source is empty",
            false,
        ));
    }
    let mime = response
        .mime_type
        .filter(|value| value.starts_with("audio/"))
        .unwrap_or_else(|| "audio/mpeg".to_owned());
    Ok((length, mime))
}

fn form_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex(byte >> 4));
            encoded.push(hex(byte & 0x0f));
        }
    }
    encoded
}

fn hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(ToOwned::to_owned)
}

fn json_u64(value: &Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_u64()
        .or_else(|| current.as_i64().map(|v| v.max(0) as u64))
        .or_else(|| current.as_str().and_then(|v| v.parse::<u64>().ok()))
}

fn json_id_string(value: Option<&Value>, fallback: String) -> String {
    match value {
        Some(Value::String(value)) if !value.is_empty() => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => fallback,
    }
}

fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| json_string(value, path))
}

fn dispatch(
    capability: &str,
    operation: &str,
    platform: &str,
    payload: &Value,
) -> Result<String, String> {
    match (capability, operation) {
        ("provider.catalog", "catalog.home") if platform == "netease" => {
            netease_catalog::home(payload)
        }
        ("provider.catalog", "catalog.discover") if platform == "netease" => {
            netease_catalog::discover(payload)
        }
        ("provider.catalog", "catalog.playlist") if platform == "netease" => {
            netease_catalog::playlist(payload)
        }
        ("provider.catalog", "catalog.album") if platform == "netease" => {
            netease_catalog::album(payload)
        }
        ("provider.catalog", "catalog.artist") if platform == "netease" => {
            netease_catalog::artist(payload)
        }
        ("provider.catalog", "catalog.artist-page") if platform == "netease" => {
            netease_catalog::artist_page(payload)
        }
        ("provider.catalog", "catalog.status") => provider_status(platform, payload),
        ("provider.catalog", "catalog.search") => search(platform, payload),
        ("provider.catalog", "catalog.song") => song_detail(platform, payload),
        ("provider.catalog", "catalog.artwork-data-uri") => artwork_data_uri(platform, payload),
        ("provider.lyrics", "lyrics.get") => lyrics(platform, payload),
        ("provider.comments", "comments.list") if platform == "netease" => {
            netease_comments::comments_page(payload)
        }
        ("provider.playback", "playback.resolve")
        | ("provider.playback", "playback.resolve-client-fallback") => playback(platform, payload),
        ("provider.account", "account.auth.login-methods") => Ok(login_methods(platform)),
        ("provider.account", "account.auth.prepare-oauth") => prepare_oauth(platform, payload),
        ("provider.account", "account.auth.start-qr") => start_qr(platform, payload),
        ("provider.account", "account.auth.complete-oauth") => complete_oauth(platform, payload),
        ("provider.account", "account.auth.heartbeat-qr") => heartbeat_qr(platform, payload),
        ("provider.account", "account.auth.cancel-oauth") => cancel_oauth(platform, payload),
        ("provider.account", "account.auth.cancel-qr") => cancel_qr(platform, payload),
        ("provider.account", "account.auth.refresh-qr") => refresh_qr(platform, payload),
        ("provider.account", "account.sign-out") => sign_out(platform, payload),
        ("provider.account", "account.snapshot") => snapshot(platform, payload),
        ("provider.account", "account.restore-session") => Ok("null".to_owned()),
        ("provider.account", "account.favorite-songs") => favorite_songs(platform, payload),
        ("provider.account", "account.playlists") => account_playlists(platform, payload),
        ("provider.account", "account.playlist-tracks") => playlist_tracks(platform, payload),
        ("provider.playback", "playback.set-preferred-quality") => {
            set_quality(platform, payload, false)
        }
        ("provider.playback", "playback.set-current-quality") => {
            set_quality(platform, payload, true)
        }
        _ => Err(unsupported(operation)),
    }
}

fn provider_id(platform: &str) -> String {
    format!("{PLUGIN_ID}.{platform}")
}

fn artwork_request_url(platform: &str, url: &str) -> Option<String> {
    if url.len() > 4096 || url.contains('#') {
        return None;
    }
    let rest = url.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    let allowed = match platform {
        "netease" => host.ends_with(".music.126.net") && !host.contains(['@', ':']),
        "kugou" => host == "imge.kugou.com",
        "spotify" => host == "i.scdn.co",
        _ => false,
    };
    if !allowed {
        return None;
    }
    Some(if platform == "netease" && !url.contains('?') {
        format!("{url}?param=300y300")
    } else {
        url.to_owned()
    })
}

fn artwork_data_uri(platform: &str, payload: &Value) -> Result<String, String> {
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .and_then(|url| artwork_request_url(platform, url))
        .ok_or_else(|| error("invalid-request", "artwork origin is not supported", false))?;
    let request = json!({"method": "GET", "url": url, "responseFormat": "image"});
    let raw = yaqmc::provider::network::request(&request.to_string())
        .map_err(|_| error("offline", "artwork request failed", true))?;
    let response: NetworkResponse<Value> = serde_json::from_str(&raw)
        .map_err(|_| error("malformed-response", "artwork response is invalid", false))?;
    if !(200..300).contains(&response.status) || !response.body.starts_with("data:image/") {
        return Err(error(
            "malformed-response",
            "artwork response is unavailable",
            true,
        ));
    }
    serde_json::to_string(&response.body).map_err(|_| {
        error(
            "malformed-response",
            "artwork response could not be encoded",
            false,
        )
    })
}

fn login_methods(platform: &str) -> String {
    match platform {
        "spotify" => json!([{
            "id": "browser-oauth",
            "label": "Spotify browser login (PKCE)",
            "flow": "oauth"
        }])
        .to_string(),
        "netease" => json!([{
            "id": "qq", "label": "QQ 登录", "flow": "oauth"
        }, {
            "id": "wechat", "label": "微信登录", "flow": "oauth"
        }, {
            "id": "qr",
            "label": "NetEase Cloud Music QR login",
            "flow": "qr"
        }])
        .to_string(),
        "kugou" => json!([{
            "id": "qq", "label": "QQ 登录", "flow": "oauth"
        }, {
            "id": "wechat", "label": "微信登录", "flow": "oauth"
        }, {
            "id": "qr",
            "label": "KuGou QR login",
            "flow": "qr"
        }])
        .to_string(),
        _ => "[]".to_owned(),
    }
}

fn state_key(platform: &str, profile: &str, name: &str) -> String {
    format!("{platform}:{profile}:{name}")
}

fn validate_quality<'quality>(
    platform: &str,
    quality: &'quality str,
) -> Result<&'quality str, String> {
    if !matches!(
        quality,
        "automatic" | "standard" | "high" | "lossless" | "hi-res" | "master"
    ) {
        return Err(error("invalid-request", "audio quality is invalid", false));
    }
    if (platform == "spotify" && !matches!(quality, "automatic" | "standard"))
        || (platform == "kugou" && quality == "master")
    {
        return Err(unsupported("audio quality"));
    }
    Ok(quality)
}

fn preferred_quality(platform: &str, profile: &str) -> Result<String, String> {
    let quality = storage_get(&state_key(platform, profile, "preferred-quality"))?
        .unwrap_or_else(|| "automatic".to_owned());
    validate_quality(platform, &quality)?;
    Ok(quality)
}

fn provider_status(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    Ok(status_value(platform, profile, &preferred_quality(platform, profile)?).to_string())
}

fn status_value(platform: &str, profile: &str, quality: &str) -> Value {
    json!({
        "providerId": provider_id(platform),
        "profileId": profile,
        "displayName": match platform {
            "spotify" => "Spotify",
            "netease" => "NetEase Cloud Music",
            _ => "KuGou Music",
        },
        "connection": "ready",
        "message": "",
        "preferredQuality": quality,
        "capabilities": {
            "search": true, "album": platform == "netease", "artist": platform == "netease", "playlist": platform == "netease",
            "lyrics": platform != "spotify", "wordTimedLyrics": false,
            "streaming": true, "qualitySelection": platform != "spotify",
        }
    })
}

fn set_quality(platform: &str, payload: &Value, current: bool) -> Result<String, String> {
    let profile = platform_id(payload);
    let quality = payload
        .get("quality")
        .and_then(Value::as_str)
        .ok_or_else(|| error("invalid-request", "audio quality is missing", false))?;
    validate_quality(platform, quality)?;
    if current {
        let id = track_id(payload);
        if id.is_empty() || id.len() > 256 {
            return Err(error("invalid-request", "track ID is invalid", false));
        }
        storage_set(
            &state_key(platform, profile, "current-quality"),
            &json!({"trackId": id, "quality": quality}).to_string(),
        )?;
        Ok("null".to_owned())
    } else {
        storage_set(&state_key(platform, profile, "preferred-quality"), quality)?;
        Ok(status_value(platform, profile, quality).to_string())
    }
}

fn playback_quality(platform: &str, payload: &Value, id: &str) -> Result<String, String> {
    if let Some(quality) = payload.get("quality").and_then(Value::as_str) {
        return validate_quality(platform, quality).map(ToOwned::to_owned);
    }
    let profile = platform_id(payload);
    let key = state_key(platform, profile, "current-quality");
    if let Some(raw) = storage_get(&key)? {
        let value: Value = serde_json::from_str(&raw)
            .map_err(|_| error("storage-failure", "audio quality state is invalid", false))?;
        if value.get("trackId").and_then(Value::as_str) == Some(id) {
            let quality = value
                .get("quality")
                .and_then(Value::as_str)
                .ok_or_else(|| error("storage-failure", "audio quality state is invalid", false))?;
            return validate_quality(platform, quality).map(ToOwned::to_owned);
        }
        storage_delete(&key)?;
    }
    preferred_quality(platform, profile)
}

fn credential_key(platform: &str, profile: &str, kind: &str) -> String {
    state_key(platform, profile, kind)
}

fn auth_header(
    platform: &str,
    profile: &str,
    kind: &str,
    header: &str,
    prefix: &str,
) -> Result<Option<(String, String, String)>, String> {
    let Some(handle) = credential_handle(&credential_key(platform, profile, kind))? else {
        return Ok(None);
    };
    Ok(Some((handle, header.to_owned(), prefix.to_owned())))
}

fn track_id(payload: &Value) -> String {
    payload
        .get("trackId")
        .or_else(|| payload.get("songId"))
        .or_else(|| payload.get("id"))
        .or_else(|| payload.pointer("/song/provider/trackId"))
        .or_else(|| payload.pointer("/song/id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .rsplit("::")
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn limit(payload: &Value) -> u64 {
    payload
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 50)
}

fn page_number(payload: &Value) -> u64 {
    payload
        .get("page")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1)
}

const SEARCH_SONG_FIELDS: u8 = 0;
const SEARCH_ARTIST_FIELDS: u8 = 1;
const SEARCH_ALBUM_FIELDS: u8 = 2;
const SEARCH_COVER_FIELDS: u8 = 3;
const SEARCH_DURATION_FIELDS: u8 = 4;
const SEARCH_IMAGE_FIELDS: u8 = 5;
const SEARCH_PRIVILEGE_FIELDS: u8 = 6;

struct CompactSearchObject<const FIELDS: u8>(Value);

type CompactSearchSong = CompactSearchObject<SEARCH_SONG_FIELDS>;

impl<'de, const FIELDS: u8> Deserialize<'de> for CompactSearchObject<FIELDS> {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        struct SearchSongVisitor<const FIELDS: u8>;

        impl<'de, const FIELDS: u8> Visitor<'de> for SearchSongVisitor<FIELDS> {
            type Value = CompactSearchObject<FIELDS>;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("a search song object or null")
            }

            fn visit_unit<Error>(self) -> Result<Self::Value, Error>
            where
                Error: serde::de::Error,
            {
                Ok(CompactSearchObject(Value::Null))
            }

            fn visit_map<Access>(self, mut access: Access) -> Result<Self::Value, Access::Error>
            where
                Access: MapAccess<'de>,
            {
                let mut fields = Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    let value = match (FIELDS, key.as_str()) {
                        (SEARCH_SONG_FIELDS, "ar" | "artists") => access
                            .next_value::<Option<Vec<CompactSearchObject<SEARCH_ARTIST_FIELDS>>>>()?
                            .map(|artists| {
                                Value::Array(artists.into_iter().map(|artist| artist.0).collect())
                            })
                            .unwrap_or(Value::Null),
                        (SEARCH_SONG_FIELDS, "al" | "album") => {
                            access
                                .next_value::<CompactSearchObject<SEARCH_ALBUM_FIELDS>>()?
                                .0
                        }
                        (SEARCH_SONG_FIELDS, "trans_param") => {
                            access
                                .next_value::<CompactSearchObject<SEARCH_COVER_FIELDS>>()?
                                .0
                        }
                        (SEARCH_SONG_FIELDS, "extra") => {
                            access
                                .next_value::<CompactSearchObject<SEARCH_DURATION_FIELDS>>()?
                                .0
                        }
                        (SEARCH_SONG_FIELDS, "privilege") => {
                            access
                                .next_value::<CompactSearchObject<SEARCH_PRIVILEGE_FIELDS>>()?
                                .0
                        }
                        (SEARCH_ALBUM_FIELDS, "images") => access
                            .next_value::<Option<Vec<CompactSearchObject<SEARCH_IMAGE_FIELDS>>>>()?
                            .map(|images| {
                                Value::Array(images.into_iter().map(|image| image.0).collect())
                            })
                            .unwrap_or(Value::Null),
                        _ if (FIELDS == SEARCH_SONG_FIELDS
                            && matches!(
                                key.as_str(),
                                "id" | "name"
                                    | "fee"
                                    | "payed"
                                    | "artists"
                                    | "album"
                                    | "duration_ms"
                                    | "preview_url"
                                    | "ar"
                                    | "al"
                                    | "dt"
                                    | "duration"
                                    | "hash"
                                    | "Hash"
                                    | "filehash"
                                    | "FileHash"
                                    | "songname"
                                    | "SongName"
                                    | "songName"
                                    | "filename"
                                    | "fileName"
                                    | "singername"
                                    | "SingerName"
                                    | "singerName"
                                    | "author_name"
                                    | "album_name"
                                    | "AlbumName"
                                    | "trans_param"
                                    | "album_img"
                                    | "imgUrl"
                                    | "extra"
                                    | "Duration"
                                    | "timeLength"
                                    | "timelen"
                                    | "timelength"
                                    | "album_id"
                                    | "AlbumID"
                                    | "albumid"
                                    | "album_audio_id"
                                    | "MixSongID"
                                    | "Singers"
                                    | "authors"
                            ))
                            || (FIELDS == SEARCH_ARTIST_FIELDS
                                && matches!(key.as_str(), "id" | "name"))
                            || (FIELDS == SEARCH_ALBUM_FIELDS
                                && matches!(key.as_str(), "id" | "name" | "picUrl"))
                            || (FIELDS == SEARCH_COVER_FIELDS && key == "union_cover")
                            || (FIELDS == SEARCH_DURATION_FIELDS && key == "128timelength")
                            || (FIELDS == SEARCH_IMAGE_FIELDS && key == "url")
                            || (FIELDS == SEARCH_PRIVILEGE_FIELDS && key == "toast") =>
                        {
                            access.next_value()?
                        }
                        _ => {
                            access.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    fields.insert(key, value);
                }
                Ok(CompactSearchObject(Value::Object(fields)))
            }
        }

        deserializer.deserialize_any(SearchSongVisitor::<FIELDS>)
    }
}

#[derive(Deserialize)]
struct SpotifySearchResponse {
    tracks: Option<SpotifySearchPage>,
}

#[derive(Deserialize)]
struct SpotifySearchPage {
    items: Option<Vec<CompactSearchSong>>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct NeteaseSearchResponse {
    code: Option<i64>,
    result: Option<NeteaseSearchPage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NeteaseSearchPage {
    songs: Option<Vec<CompactSearchSong>>,
    song_count: Option<u64>,
}

#[derive(Deserialize)]
struct KugouSearchResponse {
    error_code: Option<i64>,
    status: Option<i64>,
    data: Option<KugouSearchPage>,
}

#[derive(Deserialize)]
struct KugouSearchPage {
    info: Option<Vec<CompactSearchSong>>,
    lists: Option<Vec<CompactSearchSong>>,
    total: Option<u64>,
}

fn search(platform: &str, payload: &Value) -> Result<String, String> {
    if platform == "netease"
        && payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("song")
            != "song"
    {
        return netease_catalog::search(payload);
    }
    if payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song")
        != "song"
    {
        return Err(unsupported("catalog.search kind"));
    }
    let query = payload
        .get("query")
        .or_else(|| payload.get("keyword"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if query.trim().is_empty() {
        return Err(error("invalid-request", "search query is empty", false));
    }
    let profile = platform_id(payload);
    let page = page_number(payload);
    let limit = limit(payload);
    let (items, has_more) = match platform {
        "spotify" => spotify_search(profile, query, page, limit)?,
        "netease" => netease_search(profile, query, page, limit)?,
        "kugou" => kugou_search(profile, query, page, limit)?,
        _ => return Err(unsupported("catalog.search")),
    };
    let mut response = json!({
        "kind": "song",
        "query": query,
        "page": page,
        "hasMore": has_more
    });
    response["items"] = Value::Array(items);
    Ok(response.to_string())
}

fn spotify_search(
    profile: &str,
    query: &str,
    page: u64,
    limit: u64,
) -> Result<(Vec<Value>, bool), String> {
    let auth = spotify_access(profile)?;
    let offset = page.saturating_sub(1).saturating_mul(limit);
    let url = query_url(
        SPOTIFY_API,
        "/v1/search",
        &[
            ("q", query.to_owned()),
            ("type", "track".to_owned()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ],
    );
    let response = request_with_meta_headers_as::<SpotifySearchResponse>(
        "GET",
        &url,
        None,
        None,
        Some((auth.0.as_str(), "authorization", "Bearer ")),
        None,
        &[],
    )?
    .body;
    let tracks = response
        .tracks
        .as_ref()
        .and_then(|tracks| tracks.items.as_deref())
        .unwrap_or(&[]);
    let items = tracks
        .iter()
        .filter_map(|item| map_spotify_song(&item.0, profile))
        .collect::<Vec<_>>();
    let has_more = response
        .tracks
        .as_ref()
        .and_then(|tracks| tracks.next.as_ref())
        .is_some();
    Ok((items, has_more))
}

fn netease_search(
    profile: &str,
    query: &str,
    page: u64,
    limit: u64,
) -> Result<(Vec<Value>, bool), String> {
    let response = netease_response::<NeteaseSearchResponse>(
        profile,
        "/api/cloudsearch/pc",
        json!({
            "s": query,
            "type": 1,
            "limit": limit,
            "offset": page.saturating_sub(1).saturating_mul(limit),
            "total": true
        }),
    )?
    .body;
    require_netease_success(&json!({"code": response.code}))?;
    let songs = response
        .result
        .as_ref()
        .and_then(|result| result.songs.as_deref())
        .unwrap_or(&[]);
    let items = songs
        .iter()
        .filter_map(|item| map_netease_song(&item.0, profile))
        .collect::<Vec<_>>();
    let has_more = response
        .result
        .as_ref()
        .and_then(|result| result.song_count)
        .map(|total| {
            page.saturating_sub(1)
                .saturating_mul(limit)
                .saturating_add(songs.len() as u64)
                < total
        })
        .unwrap_or(songs.len() as u64 >= limit);
    Ok((items, has_more))
}

fn kugou_search(
    profile: &str,
    query: &str,
    page: u64,
    limit: u64,
) -> Result<(Vec<Value>, bool), String> {
    let client_time_ms = now_ms();
    let client_time = (client_time_ms / 1000).to_string();
    let mid = md5_hex(&client_time_ms.to_string()).to_ascii_lowercase();
    let mut params = [
        ("sorttype", "0".to_owned()),
        ("keyword", query.to_owned()),
        ("page", page.to_string()),
        ("pagesize", limit.to_string()),
        ("userid", "0".to_owned()),
        ("appid", "3116".to_owned()),
        ("token", String::new()),
        ("clienttime", client_time),
        ("iscorrection", "1".to_owned()),
        ("uuid", "-".to_owned()),
        ("mid", mid.clone()),
        ("dfid", "-".to_owned()),
        ("clientver", "11070".to_owned()),
        ("platform", "AndroidFilter".to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect::<Vec<_>>();
    let signature = md5_sorted_signature(&params, "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA", false, "")
        .to_ascii_lowercase();
    params.push(("signature".to_owned(), signature));
    let url = query_url(
        KUGOU_API,
        "/v2/search/song",
        &params
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>(),
    );
    let response = request_with_meta_headers_as::<KugouSearchResponse>(
        "GET",
        &url,
        None,
        None,
        None,
        None,
        &[
            (
                "user-agent",
                "Android14-1070-11070-201-0-SearchSong-wifi".to_owned(),
            ),
            ("kg-rec", "1".to_owned()),
            ("kg-rc", "1".to_owned()),
            ("mid", mid),
            ("x-router", "complexsearch.kugou.com".to_owned()),
        ],
    )?
    .body;
    if response
        .error_code
        .is_some_and(|code| code != 0 && code != 200)
        || response.status.is_some_and(|status| status == 0)
    {
        return Err(error(
            "provider-unavailable",
            "KuGou rejected the request",
            true,
        ));
    }
    let songs = response
        .data
        .as_ref()
        .and_then(|data| data.info.as_deref().or(data.lists.as_deref()))
        .unwrap_or(&[]);
    let items = songs
        .iter()
        .filter_map(|item| map_kugou_song(&item.0, profile))
        .collect::<Vec<_>>();
    let has_more = response
        .data
        .as_ref()
        .and_then(|data| data.total)
        .map(|total| {
            page.saturating_sub(1)
                .saturating_mul(limit)
                .saturating_add(songs.len() as u64)
                < total
        })
        .unwrap_or(songs.len() as u64 >= limit);
    Ok((items, has_more))
}

fn map_spotify_song(item: &Value, profile: &str) -> Option<Value> {
    let id = item.get("id").and_then(Value::as_str)?.to_owned();
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
    let artists = item
        .get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    json!({
                        "id": json_id_string(artist.get("id"), format!("artist-{index}")),
                        "name": artist.get("name").cloned().unwrap_or(json!("Unknown"))
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let album = item.get("album").cloned().unwrap_or(Value::Null);
    let artwork = album
        .get("images")
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .and_then(|image| image.get("url"))
        .cloned()
        .unwrap_or(json!(""));
    let preview = item.get("preview_url").cloned().unwrap_or(Value::Null);
    Some(song_value(
        "spotify",
        profile,
        &id,
        title,
        artists,
        album
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown album"),
        artwork,
        item.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
        preview.clone(),
        preview.as_str().is_some_and(|value| !value.is_empty()),
    ))
}

fn map_netease_song(item: &Value, profile: &str) -> Option<Value> {
    let id = item.get("id").and_then(|value| {
        value
            .as_i64()
            .map(|id| id.to_string())
            .or_else(|| value.as_str().map(ToOwned::to_owned))
    })?;
    let artists = item
        .get("ar")
        .or_else(|| item.get("artists"))
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    json!({
                        "id": json_id_string(artist.get("id"), format!("artist-{index}")),
                        "name": artist.get("name").cloned().unwrap_or(json!("Unknown"))
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let album = item.get("al").or_else(|| item.get("album"));
    let mut song = song_value(
        "netease",
        profile,
        &id,
        item.get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled"),
        artists,
        album
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Unknown album"),
        album
            .and_then(|value| value.get("picUrl"))
            .and_then(Value::as_str)
            .map(normalize_netease_artwork)
            .map(Value::String)
            .unwrap_or(json!("")),
        item.get("dt")
            .and_then(Value::as_u64)
            .or_else(|| item.get("duration").and_then(Value::as_u64))
            .unwrap_or(0),
        Value::Null,
        true,
    );
    // Catalog billing describes the track, not this account's actual playback
    // entitlement. Keep it visible without preventing an authorized resolve.
    match json_u64(item, &["fee"]) {
        Some(1) => {
            song["availability"] =
                json!({"status":"entitlement-required", "requiredTier":"music-vip"})
        }
        Some(4) => {
            song["availability"] =
                json!({"status":"entitlement-required", "requiredTier":"purchase"})
        }
        _ => {}
    }
    if item.pointer("/privilege/toast").and_then(Value::as_bool) == Some(true) {
        song["availability"] = json!({"status":"unavailable", "reason":"copyright-restricted"});
    }
    song.as_object_mut()?.remove("playbackCapability");
    Some(song)
}

fn map_kugou_song(item: &Value, profile: &str) -> Option<Value> {
    let item = item
        .get("fileinfo")
        .filter(|value| value.is_object())
        .unwrap_or(item);
    let id = first_string(item, &[&["hash"], &["Hash"], &["filehash"], &["FileHash"]])?;
    let title = first_string(
        item,
        &[
            &["songname"],
            &["SongName"],
            &["songName"],
            &["filename"],
            &["fileName"],
        ],
    )
    .unwrap_or_else(|| "Untitled".to_owned());
    let artist = first_string(
        item,
        &[
            &["singername"],
            &["SingerName"],
            &["singerName"],
            &["author_name"],
        ],
    )
    .unwrap_or_else(|| "Unknown".to_owned());
    let album = first_string(item, &[&["album_name"], &["AlbumName"]])
        .unwrap_or_else(|| "Unknown album".to_owned());
    let artwork = item
        .get("trans_param")
        .and_then(|value| value.get("union_cover"))
        .or_else(|| item.get("album_img"))
        .or_else(|| item.get("imgUrl"))
        .and_then(Value::as_str)
        .map(normalize_kugou_artwork)
        .map(Value::String)
        .unwrap_or(json!(""));
    let duration = json_u64(item, &["extra", "128timelength"])
        .filter(|value| *value > 0)
        .or_else(|| json_u64(item, &["timelen"]).filter(|value| *value > 0))
        .or_else(|| json_u64(item, &["timelength"]).filter(|value| *value > 0))
        .or_else(|| {
            json_u64(item, &["duration"])
                .filter(|value| *value > 0)
                .map(|value| value.saturating_mul(1000))
        })
        .or_else(|| {
            json_u64(item, &["Duration"])
                .filter(|value| *value > 0)
                .map(|value| value.saturating_mul(1000))
        })
        .or_else(|| {
            json_u64(item, &["timeLength"])
                .filter(|value| *value > 0)
                .map(|value| value.saturating_mul(1000))
        })
        .unwrap_or(0);
    let artists = item
        .get("Singers")
        .or_else(|| item.get("authors"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    let name = first_string(value, &[&["name"], &["author_name"]])?;
                    let id = json_id_string(
                        value.get("id").or_else(|| value.get("author_id")),
                        format!("{id}-artist"),
                    );
                    Some(json!({"id":id,"name":name}))
                })
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![json!({"id": format!("{id}-artist"), "name": artist})]);
    let mut song = song_value(
        "kugou",
        profile,
        &id,
        &title,
        artists,
        &album,
        artwork,
        duration,
        Value::Null,
        true,
    );
    // Catalog metadata is not proof that this account can play the full track.
    song.as_object_mut()?.remove("playbackCapability");
    match kugou_notice_code(item) {
        Some("entitlement-insufficient") => {
            song["availability"] =
                json!({"status":"entitlement-required","requiredTier":"music-vip"})
        }
        Some("copyright-restricted") => {
            song["availability"] = json!({"status":"unavailable","reason":"copyright-restricted"})
        }
        _ => {}
    }
    if let Some(album_id) = ["AlbumID", "album_id", "albumid"]
        .iter()
        .find_map(|key| item.get(*key).and_then(|value| json_u64(value, &[])))
        .filter(|id| *id > 0)
    {
        song["album"]["id"] = json!(album_id.to_string());
        song["provider"]["albumId"] = json!(album_id.to_string());
    }
    if let Some(mixsong_id) = ["MixSongID", "album_audio_id"]
        .iter()
        .find_map(|key| item.get(*key).and_then(|value| json_u64(value, &[])))
        .filter(|id| *id > 0)
    {
        song["provider"]["numericId"] = json!(mixsong_id);
    }
    Some(song)
}

fn normalize_netease_artwork(value: &str) -> String {
    if let Some(remainder) = value.strip_prefix("http://") {
        let upgraded = format!("https://{remainder}");
        if artwork_request_url("netease", &upgraded).is_some() {
            return upgraded;
        }
    }
    value.to_owned()
}

fn normalize_kugou_artwork(value: &str) -> String {
    let value = value.replace("{size}", "400");
    for host in ["imge.kugou.com", "singerimg.kugou.com"] {
        let prefix = format!("http://{host}/");
        if let Some(path) = value.strip_prefix(&prefix) {
            return format!("https://{host}/{path}");
        }
    }
    value
}

#[expect(
    clippy::too_many_arguments,
    reason = "Provider DTO projection keeps its wire fields explicit"
)]
fn song_value(
    platform: &str,
    profile: &str,
    id: &str,
    title: &str,
    artists: Vec<Value>,
    album: &str,
    artwork: Value,
    duration: u64,
    preview: Value,
    playable: bool,
) -> Value {
    let preview_available = preview.as_str().is_some_and(|value| !value.is_empty());
    let playable = playable && (preview_available || platform != "spotify");
    let mut song = json!({
        "id": id,
        "title": title,
        "album": {"id": format!("{platform}-album"), "title": album},
        "artwork": {"alt": title, "dominantColor": "#334155", "variants": []},
        "durationMs": duration,
        "trackNumber": 0,
        "isFavorite": false,
        "quality": "standard",
        "availability": if playable { json!({"status":"available"}) } else { json!({"status":"unavailable", "reason":"source-unavailable"}) },
        "playbackCapability": if preview_available { json!({"status":"preview", "startMs":0, "endMs":30000}) } else if playable { json!({"status":"full"}) } else { json!({"status":"unavailable", "reason":"source-unavailable"}) },
        "provider": {"providerId": provider_id(platform), "profileId": profile, "trackId": id}
    });
    song["artists"] = Value::Array(artists);
    song["artwork"]["src"] = artwork;
    song["_previewUrl"] = preview;
    song
}

fn spotify_client_id() -> Result<String, String> {
    setting("spotifyClientId")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            error(
                "provider-not-configured",
                "Spotify Client ID must be configured in the plugin settings",
                false,
            )
        })
}

fn spotify_access(profile: &str) -> Result<(String, u64), String> {
    let handle_key = credential_key("spotify", profile, "access-token");
    let expiry_key = state_key("spotify", profile, "access-expires");
    let handle = credential_handle(&handle_key)?;
    let expires = storage_get(&expiry_key)?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if let Some(handle) = handle {
        if expires > now_ms().saturating_add(30_000) {
            return Ok((handle, expires));
        }
    }
    spotify_refresh(profile)
}

fn spotify_refresh(profile: &str) -> Result<(String, u64), String> {
    let client_id = spotify_client_id()?;
    let refresh_key = credential_key("spotify", profile, "refresh-token");
    let refresh = credential_handle(&refresh_key)?.ok_or_else(|| {
        error(
            "authentication-expired",
            "Spotify authorization is required",
            false,
        )
    })?;
    let body = format!(
        "grant_type=refresh_token&client_id={}",
        form_encode(&client_id)
    );
    let response = request(
        "POST",
        &format!("{SPOTIFY_ACCOUNTS}/api/token"),
        Some(&body),
        Some("application/x-www-form-urlencoded"),
        None,
        Some((&refresh, "refresh_token")),
    )?;
    store_spotify_tokens(profile, &response, false)
}

fn store_spotify_tokens(
    profile: &str,
    response: &Value,
    require_refresh: bool,
) -> Result<(String, u64), String> {
    let (access_token, refresh_token, expires_in) =
        spotify_token_response(response, require_refresh)?;
    let access_key = credential_key("spotify", profile, "access-token");
    let access_handle = replace_credential(&access_key, SPOTIFY_API, access_token)?;
    if let Some(refresh_token) = refresh_token {
        let refresh_key = credential_key("spotify", profile, "refresh-token");
        replace_credential(&refresh_key, SPOTIFY_ACCOUNTS, refresh_token)?;
    }
    let expires = now_ms().saturating_add(expires_in.saturating_mul(1000));
    storage_set(
        &state_key("spotify", profile, "access-expires"),
        &expires.to_string(),
    )?;
    Ok((access_handle, expires))
}

fn spotify_token_response(
    response: &Value,
    require_refresh: bool,
) -> Result<(&str, Option<&str>, u64), String> {
    let malformed = || {
        error(
            "malformed-response",
            "Spotify returned an incomplete token response",
            false,
        )
    };
    let access = response
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(malformed)?;
    let refresh = response
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    if require_refresh && refresh.is_none() {
        return Err(malformed());
    }
    if !response
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("Bearer"))
    {
        return Err(malformed());
    }
    let expires = response
        .get("expires_in")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0 && *value <= 86400)
        .ok_or_else(malformed)?;
    Ok((access, refresh, expires))
}

fn prepare_oauth(platform: &str, payload: &Value) -> Result<String, String> {
    if platform != "spotify" {
        return prepare_browser_login(platform, payload);
    }
    let client_id = spotify_client_id()?;
    let profile = platform_id(payload);
    let attempt_id = payload
        .get("attemptId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("invalid-request", "OAuth attempt ID is missing", false))?;
    let state = payload
        .get("state")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("invalid-request", "OAuth state is missing", false))?;
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes(48)?);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let expires_at = now_ms().saturating_add(10 * 60 * 1000);
    let attempt = json!({
        "attemptId": attempt_id,
        "state": state,
        "verifier": verifier,
        "expiresAtMs": expires_at
    });
    storage_set(
        &state_key("spotify", profile, "oauth-attempt"),
        &attempt.to_string(),
    )?;
    let scope = "user-read-private user-read-email user-library-read playlist-read-private playlist-read-collaborative";
    let url = query_url(
        SPOTIFY_ACCOUNTS,
        "/authorize",
        &[
            ("client_id", client_id),
            ("response_type", "code".to_owned()),
            ("redirect_uri", SPOTIFY_CALLBACK.to_owned()),
            ("state", state.to_owned()),
            ("code_challenge_method", "S256".to_owned()),
            ("code_challenge", challenge),
            ("scope", scope.to_owned()),
        ],
    );
    Ok(json!({
        "url": url,
        "navigationAllowlist": [format!("{SPOTIFY_ACCOUNTS}/**")],
        "callbackMatcher": {"urlPrefix": SPOTIFY_CALLBACK}
    })
    .to_string())
}

fn complete_oauth(platform: &str, payload: &Value) -> Result<String, String> {
    if platform != "spotify" {
        return complete_browser_login(platform, payload);
    }
    let profile = platform_id(payload);
    let attempt_id = payload
        .get("attemptId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let callback = payload
        .get("callbackUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| error("invalid-request", "OAuth callback URL is missing", false))?;
    let stored =
        storage_get(&state_key("spotify", profile, "oauth-attempt"))?.ok_or_else(|| {
            error(
                "invalid-request",
                "OAuth attempt is no longer active",
                false,
            )
        })?;
    let attempt: Value = serde_json::from_str(&stored).map_err(|_| {
        error(
            "storage-failure",
            "OAuth attempt could not be restored",
            false,
        )
    })?;
    if attempt.get("attemptId").and_then(Value::as_str) != Some(attempt_id)
        || attempt
            .get("expiresAtMs")
            .and_then(Value::as_u64)
            .is_some_and(|expires| expires <= now_ms())
    {
        return Err(error(
            "authorization-rejected",
            "OAuth attempt is invalid or expired",
            false,
        ));
    }
    let state = query_parameter(callback, "state").unwrap_or_default();
    if attempt.get("state").and_then(Value::as_str) != Some(state.as_str()) {
        return Err(error(
            "authorization-rejected",
            "OAuth state did not match",
            false,
        ));
    }
    let code = query_parameter(callback, "code")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error(
                "authorization-rejected",
                "Spotify authorization was rejected",
                false,
            )
        })?;
    let verifier = attempt
        .get("verifier")
        .and_then(Value::as_str)
        .ok_or_else(|| error("storage-failure", "OAuth verifier is unavailable", false))?;
    let client_id = spotify_client_id()?;
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        form_encode(&code),
        form_encode(SPOTIFY_CALLBACK),
        form_encode(&client_id),
        form_encode(verifier),
    );
    let response = request(
        "POST",
        &format!("{SPOTIFY_ACCOUNTS}/api/token"),
        Some(&body),
        Some("application/x-www-form-urlencoded"),
        None,
        None,
    )?;
    store_spotify_tokens(profile, &response, true)?;
    storage_delete(&state_key("spotify", profile, "oauth-attempt"))?;
    spotify_account_snapshot(profile)
}

fn cancel_oauth(platform: &str, payload: &Value) -> Result<String, String> {
    if !["spotify", "netease", "kugou"].contains(&platform) {
        return Err(unsupported("account.auth.cancel-oauth"));
    }
    let profile = platform_id(payload);
    let key = state_key(platform, profile, "oauth-attempt");
    let Some(raw) = storage_get(&key)? else {
        return snapshot(platform, payload);
    };
    let attempt: Value = serde_json::from_str(&raw).map_err(|_| {
        error(
            "storage-failure",
            "OAuth attempt could not be restored",
            false,
        )
    })?;
    let attempt_id = payload.get("attemptId").and_then(Value::as_str);
    if attempt_id.is_none_or(|id| attempt.get("attemptId").and_then(Value::as_str) != Some(id)) {
        return Err(error(
            "invalid-request",
            "OAuth attempt is no longer active",
            false,
        ));
    }
    storage_delete(&key)?;
    guest_snapshot(
        platform,
        profile,
        "cancelled",
        payload.get("attemptId").cloned(),
    )
}

fn browser_login_configuration(platform: &str, method: &str, now: u64) -> Result<Value, String> {
    if !["qq", "wechat"].contains(&method) || !["netease", "kugou"].contains(&platform) {
        return Err(unsupported("account.auth.prepare-oauth"));
    }
    let (url, source, callback, names, origins, format) = if platform == "netease" {
        (
            format!("https://music.163.com/api/sns/authorize?snsType={}&clientType=web2&callbackType=Login&forcelogin=true", if method == "qq" {5} else {10}),
            "https://music.163.com/",
            format!("https://music.163.com/back/{}", if method == "qq" {"qq"} else {"weichat"}),
            vec!["MUSIC_U", "__csrf"],
            vec![NETEASE_API],
            "cookie",
        )
    } else {
        (
            format!("https://openplat-user.kugou.com/{}?appid=1014&ktype=0&clienttime={}&callback=kgLoginCallback&domain=kugou.com", if method == "qq" {"qq/"} else {"weixin/index.php"}, now / 1000),
            "https://openplat-user.kugou.com/",
            format!("https://openplat-user.kugou.com/{}_callback", if method == "qq" {"qq"} else {"wechat"}),
            vec!["KuGoo"],
            vec![KUGOU_API, KUGOU_LOGIN, KUGOU_MEDIA],
            "kugou-web",
        )
    };
    let mut allowlist = vec![format!("{source}**")];
    if platform == "kugou" {
        allowlist.push("https://cross.kugou.com/**".to_owned());
    }
    for origin in [
        "https://graph.qq.com",
        "https://xui.ptlogin2.qq.com",
        "https://ssl.ptlogin2.qq.com",
        "https://ui.ptlogin2.qq.com",
        "https://open.weixin.qq.com",
    ] {
        allowlist.push(format!("{origin}/**"));
    }
    Ok(json!({
        "url": url,
        "navigationAllowlist": allowlist,
        "callbackMatcher": {"urlPrefix": callback},
        "browserSession": {
            "url": source,
            "cookieNames": names,
            "requiredCookieNames": [if platform == "netease" {"MUSIC_U"} else {"KuGoo"}],
            "credentialOrigins": origins,
            "format": format,
        }
    }))
}

fn prepare_browser_login(platform: &str, payload: &Value) -> Result<String, String> {
    let method = payload
        .get("loginProvider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let prepared = browser_login_configuration(platform, method, now_ms())?;
    let attempt = payload
        .get("attemptId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| error("invalid-request", "OAuth attempt ID is missing", false))?;
    storage_set(&state_key(platform, platform_id(payload), "oauth-attempt"), &json!({
        "attemptId": attempt, "method": method, "expiresAtMs": now_ms().saturating_add(10 * 60 * 1000)
    }).to_string())?;
    Ok(prepared.to_string())
}

fn complete_browser_login(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    let key = state_key(platform, profile, "oauth-attempt");
    let raw = storage_get(&key)?.ok_or_else(|| {
        error(
            "invalid-request",
            "OAuth attempt is no longer active",
            false,
        )
    })?;
    let attempt: Value = serde_json::from_str(&raw).map_err(|_| {
        error(
            "storage-failure",
            "OAuth attempt could not be restored",
            false,
        )
    })?;
    if payload.get("attemptId").and_then(Value::as_str).is_none()
        || payload.get("attemptId") != attempt.get("attemptId")
        || json_u64(&attempt, &["expiresAtMs"]).unwrap_or(0) <= now_ms()
    {
        return Err(error(
            "authorization-rejected",
            "OAuth attempt is invalid or expired",
            false,
        ));
    }
    let origins: &[(&str, &str)] = match platform {
        "netease" => &[(NETEASE_API, "cookie")],
        "kugou" => &[
            (KUGOU_API, "cookie-api"),
            (KUGOU_LOGIN, "cookie-login"),
            (KUGOU_MEDIA, "cookie-media"),
        ],
        _ => return Err(unsupported("account.auth.complete-oauth")),
    };
    let credentials = browser_credential_handles(payload, origins)?;
    let handle = &credentials[0].1;
    let response = if platform == "netease" {
        let path = "/api/w/nuser/account/get";
        request(
            "POST",
            &netease_eapi_url(path)?,
            Some(&format!(
                "params={}",
                netease_encrypted_params(path, &json!({}))
            )),
            Some("application/x-www-form-urlencoded"),
            Some((handle, "cookie", "")),
            None,
        )?
    } else {
        kugou_request(
            "POST",
            KUGOU_API,
            "/v3/get_my_info",
            &[("plat", "1".to_owned())],
            Some(handle),
            KugouSignature::Android,
            Some("usercenter.kugou.com"),
            false,
        )?
    };
    let display_name = account_display_name(platform, &response)?;
    let previous = credentials
        .iter()
        .map(|(kind, _)| credential_handle(&credential_key(platform, profile, kind)))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, (kind, handle)) in credentials.iter().enumerate() {
        if let Err(reason) = storage_set(&credential_key(platform, profile, kind), handle) {
            for (offset, (kind, _)) in credentials[..index].iter().enumerate() {
                let old_key = credential_key(platform, profile, kind);
                if let Some(old) = &previous[offset] {
                    let _ = storage_set(&old_key, old);
                } else {
                    let _ = storage_delete(&old_key);
                }
            }
            return Err(reason);
        }
    }
    if let Err(reason) = storage_delete(&key) {
        for ((kind, _), old) in credentials.iter().zip(previous.iter()) {
            let old_key = credential_key(platform, profile, kind);
            if let Some(old) = old {
                let _ = storage_set(&old_key, old);
            } else {
                let _ = storage_delete(&old_key);
            }
        }
        return Err(reason);
    }
    for old in previous.into_iter().flatten() {
        let _ = yaqmc::provider::credentials::delete(&old);
    }
    if platform == "netease" {
        netease_catalog::account(profile, &response)
    } else if platform == "kugou" {
        kugou_account(profile, &response)
    } else {
        authenticated_snapshot(platform, profile, display_name)
    }
}

fn browser_credential_handles(
    payload: &Value,
    origins: &[(&str, &str)],
) -> Result<Vec<(String, String)>, String> {
    let values = payload
        .get("browserCredentials")
        .and_then(Value::as_array)
        .filter(|values| values.len() == origins.len())
        .ok_or_else(|| {
            error(
                "invalid-request",
                "browser credentials are incomplete",
                false,
            )
        })?;
    origins
        .iter()
        .map(|(origin, kind)| {
            let matches = values
                .iter()
                .filter(|item| item.get("origin").and_then(Value::as_str) == Some(origin))
                .collect::<Vec<_>>();
            let handle = matches
                .first()
                .filter(|_| matches.len() == 1)
                .and_then(|item| item.get("handle"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    error("invalid-request", "browser credentials are invalid", false)
                })?;
            Ok(((*kind).to_owned(), handle.to_owned()))
        })
        .collect()
}

fn start_qr(platform: &str, payload: &Value) -> Result<String, String> {
    if platform == "spotify" {
        return Err(unsupported("account.auth.start-qr"));
    }
    let profile = platform_id(payload).to_owned();
    let attempt_id = random_token("qr_")?;
    let owner_lease_id = random_token("lease_")?;
    let (key, image, launch_url) = match platform {
        "netease" => netease_prepare_qr(&profile)?,
        "kugou" => kugou_prepare_qr()?,
        _ => return Err(unsupported("account.auth.start-qr")),
    };
    let expires_at = now_ms().saturating_add(5 * 60 * 1000);
    let state = json!({
        "attemptId": attempt_id,
        "ownerLeaseId": owner_lease_id,
        "key": key,
        "qrImageDataUri": image,
        "launchUrl": launch_url,
        "expiresAtMs": expires_at,
        "phase": "waiting-for-scan"
    });
    storage_set(
        &state_key(platform, &profile, "qr-attempt"),
        &state.to_string(),
    )?;
    qr_snapshot(platform, &profile, &state, "waiting-for-scan")
}

fn netease_prepare_qr(profile: &str) -> Result<(String, String, Option<String>), String> {
    let key_response = netease_request(profile, "/api/login/qrcode/unikey", json!({"type": 3}))?;
    require_netease_success(&key_response)?;
    let key = first_string(&key_response, &[&["unikey"], &["data", "unikey"], &["key"]])
        .ok_or_else(|| error("malformed-response", "NetEase returned no QR key", true))?;
    let launch_url = format!("https://music.163.com/login?codekey={key}");
    let image = qr_png_data_uri(&launch_url)
        .ok_or_else(|| error("malformed-response", "NetEase returned no QR image", true))?;
    Ok((key, image, Some(launch_url)))
}

fn kugou_prepare_qr() -> Result<(String, String, Option<String>), String> {
    let response = kugou_request(
        "GET",
        KUGOU_LOGIN,
        "/v2/qrcode",
        &[
            ("appid", "1001".to_owned()),
            ("type", "1".to_owned()),
            ("plat", "4".to_owned()),
            (
                "qrcode_txt",
                "https://h5.kugou.com/apps/loginQRCode/html/index.html?appid=1005&".to_owned(),
            ),
            ("srcappid", "2919".to_owned()),
        ],
        None,
        KugouSignature::Web,
        None,
        false,
    )?;
    let key = first_string(&response, &[&["data", "qrcode"], &["data", "key"]])
        .ok_or_else(|| error("malformed-response", "KuGou returned no QR key", true))?;
    let launch = format!("https://h5.kugou.com/apps/loginQRCode/html/index.html?qrcode={key}");
    let image = qr_png_data_uri(&launch).ok_or_else(|| {
        error(
            "malformed-response",
            "KuGou QR image could not be created",
            true,
        )
    })?;
    Ok((key, image, Some(launch)))
}

fn heartbeat_qr(platform: &str, payload: &Value) -> Result<String, String> {
    if platform == "spotify" {
        return Err(unsupported("account.auth.heartbeat-qr"));
    }
    let profile = platform_id(payload).to_owned();
    let key = state_key(platform, &profile, "qr-attempt");
    let Some(raw) = storage_get(&key)? else {
        return snapshot(platform, payload);
    };
    let state: Value = serde_json::from_str(&raw)
        .map_err(|_| error("storage-failure", "QR login state is invalid", false))?;
    if payload.get("attemptId").and_then(Value::as_str).is_none()
        || payload
            .get("ownerLeaseId")
            .and_then(Value::as_str)
            .is_none()
        || !qr_attempt_matches(&state, payload)
    {
        return Err(error(
            "invalid-request",
            "the QR login attempt or owner lease is no longer active",
            false,
        ));
    }
    if json_u64(&state, &["expiresAtMs"]).unwrap_or(0) <= now_ms() {
        storage_delete(&key)?;
        return qr_snapshot(platform, &profile, &state, "expired");
    }
    let qr_key = json_string(&state, &["key"]).unwrap_or_default();
    let result = match platform {
        "netease" => netease_check_qr(&qr_key, &profile),
        "kugou" => kugou_check_qr(&qr_key, &profile),
        _ => Err(unsupported("account.auth.heartbeat-qr")),
    };
    finish_qr_poll(platform, &profile, state, result)
}

fn finish_qr_poll(
    platform: &str,
    profile: &str,
    mut state: Value,
    result: Result<QrCheck, String>,
) -> Result<String, String> {
    // Keep the persisted attempt for retry, but let the UI show the error.
    // Returning the last waiting state made failed polling look successful.
    let result = result?;
    let key = state_key(platform, profile, "qr-attempt");
    match result {
        QrCheck::WaitingScan => {
            state["phase"] = json!("waiting-for-scan");
            storage_set(&key, &state.to_string())?;
            qr_snapshot(platform, profile, &state, "waiting-for-scan")
        }
        QrCheck::WaitingConfirmation => {
            state["phase"] = json!("waiting-for-confirmation");
            storage_set(&key, &state.to_string())?;
            qr_snapshot(platform, profile, &state, "waiting-for-confirmation")
        }
        QrCheck::Authenticated {
            secret,
            display_name,
            avatar_url,
        } => {
            if platform == "netease" {
                replace_credential(
                    &credential_key(platform, profile, "cookie"),
                    NETEASE_API,
                    &secret,
                )?;
            } else {
                replace_credential(
                    &credential_key(platform, profile, "cookie-login"),
                    KUGOU_LOGIN,
                    &secret,
                )?;
                replace_credential(
                    &credential_key(platform, profile, "cookie-api"),
                    KUGOU_API,
                    &secret,
                )?;
                replace_credential(
                    &credential_key(platform, profile, "cookie-media"),
                    KUGOU_MEDIA,
                    &secret,
                )?;
            }
            storage_delete(&key)?;
            let mut snapshot =
                authenticated_snapshot_value(platform, profile, display_name, now_ms());
            snapshot["profile"]["avatarUrl"] = json!(avatar_url);
            Ok(snapshot.to_string())
        }
        QrCheck::Expired => {
            storage_delete(&key)?;
            qr_snapshot(platform, profile, &state, "expired")
        }
    }
}

enum QrCheck {
    WaitingScan,
    WaitingConfirmation,
    Authenticated {
        secret: String,
        display_name: Option<String>,
        avatar_url: Option<String>,
    },
    Expired,
}

fn qr_attempt_matches(state: &Value, payload: &Value) -> bool {
    let attempt = payload.get("attemptId").and_then(Value::as_str);
    let lease = payload.get("ownerLeaseId").and_then(Value::as_str);
    attempt.is_some_and(|value| state.get("attemptId").and_then(Value::as_str) == Some(value))
        && lease
            .is_none_or(|value| state.get("ownerLeaseId").and_then(Value::as_str) == Some(value))
}

fn netease_check_qr(key: &str, _profile: &str) -> Result<QrCheck, String> {
    let response = netease_request(
        _profile,
        "/api/login/qrcode/client/login",
        json!({"key": key, "type": 3}),
    )?;
    match response.get("code").and_then(Value::as_i64).unwrap_or(0) {
        801 => Ok(QrCheck::WaitingScan),
        802 => Ok(QrCheck::WaitingConfirmation),
        803 => {
            let secret =
                first_string(&response, &[&["cookie"], &["data", "cookie"]]).ok_or_else(|| {
                    error(
                        "malformed-response",
                        "NetEase returned no login cookie",
                        false,
                    )
                })?;
            Ok(QrCheck::Authenticated {
                secret,
                display_name: None,
                avatar_url: None,
            })
        }
        800 => Ok(QrCheck::Expired),
        _ => Err(error(
            "malformed-response",
            "NetEase returned an unknown QR status",
            true,
        )),
    }
}

fn kugou_check_qr(key: &str, _profile: &str) -> Result<QrCheck, String> {
    let response = kugou_request(
        "GET",
        KUGOU_LOGIN,
        "/v2/get_userinfo_qrcode",
        &[
            ("plat", "4".to_owned()),
            ("appid", "1005".to_owned()),
            ("srcappid", "2919".to_owned()),
            ("qrcode", key.to_owned()),
            ("dev", "-".to_owned()),
        ],
        None,
        KugouSignature::Web,
        None,
        false,
    )?;
    parse_kugou_qr_check(&response)
}

fn parse_kugou_qr_check(response: &Value) -> Result<QrCheck, String> {
    match json_u64(response, &["data", "status"]) {
        Some(1) => Ok(QrCheck::WaitingScan),
        Some(2) => Ok(QrCheck::WaitingConfirmation),
        Some(4) => {
            let token = json_string(response, &["data", "token"]).ok_or_else(|| {
                error("malformed-response", "KuGou returned no login token", false)
            })?;
            let userid = first_string(response, &[&["data", "userid"], &["data", "user_id"]])
                .or_else(|| {
                    json_u64(response, &["data", "userid"])
                        .or_else(|| json_u64(response, &["data", "user_id"]))
                        .map(|v| v.to_string())
                })
                .ok_or_else(|| error("malformed-response", "KuGou returned no user ID", false))?;
            if token.is_empty()
                || token.len() > 8192
                || token
                    .bytes()
                    .any(|byte| !(0x21..=0x7e).contains(&byte) || matches!(byte, b';' | b','))
                || !userid.parse::<u64>().is_ok_and(|value| value > 0)
            {
                return Err(error(
                    "malformed-response",
                    "KuGou returned invalid login credentials",
                    false,
                ));
            }
            Ok(QrCheck::Authenticated {
                secret: format!("token={token}; userid={userid}"),
                display_name: json_string(response, &["data", "nickname"]),
                avatar_url: kugou_avatar(response),
            })
        }
        Some(0) => Ok(QrCheck::Expired),
        _ => Err(error(
            "malformed-response",
            "KuGou returned an unknown QR status",
            true,
        )),
    }
}

fn cookies_from_list(cookies: &[String]) -> Option<String> {
    let value = cookies
        .iter()
        .map(|cookie| cookie.split(';').next().unwrap_or(cookie))
        .filter(|cookie| !cookie.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!value.is_empty()).then_some(value)
}

fn cancel_qr(platform: &str, payload: &Value) -> Result<String, String> {
    if platform == "spotify" {
        return Err(unsupported("account.auth.cancel-qr"));
    }
    let profile = platform_id(payload).to_owned();
    let key = state_key(platform, &profile, "qr-attempt");
    if let Some(raw) = storage_get(&key)? {
        let state: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        if qr_attempt_matches(&state, payload) {
            storage_delete(&key)?;
            return qr_snapshot(platform, &profile, &state, "cancelled");
        }
        return Err(error(
            "invalid-request",
            "the QR login attempt or owner lease is no longer active",
            false,
        ));
    }
    snapshot(platform, payload)
}

fn refresh_qr(platform: &str, payload: &Value) -> Result<String, String> {
    if platform == "spotify" {
        return Err(unsupported("account.auth.refresh-qr"));
    }
    let profile = platform_id(payload);
    let key = state_key(platform, profile, "qr-attempt");
    if let Some(raw) = storage_get(&key)? {
        let state: Value = serde_json::from_str(&raw)
            .map_err(|_| error("storage-failure", "QR login state is invalid", false))?;
        if payload.get("attemptId").and_then(Value::as_str).is_some()
            && !qr_attempt_matches(&state, payload)
        {
            return Err(error(
                "invalid-request",
                "the QR login attempt or owner lease is no longer active",
                false,
            ));
        }
    }
    storage_delete(&key)?;
    start_qr(platform, payload)
}

fn qr_snapshot_from_state(platform: &str, profile: &str, state: &Value) -> Result<String, String> {
    let phase = json_string(state, &["phase"]).unwrap_or_else(|| "waiting-for-scan".to_owned());
    qr_snapshot(platform, profile, state, &phase)
}

fn qr_snapshot(
    platform: &str,
    profile: &str,
    state: &Value,
    phase: &str,
) -> Result<String, String> {
    let state_name = match phase {
        "waiting-for-confirmation" => "waiting-for-confirmation",
        "expired" => "expired",
        "cancelled" => "cancelled",
        "rejected" => "rejected",
        "network-error" => "network-error",
        _ => "waiting-for-scan",
    };
    let active = matches!(state_name, "waiting-for-scan" | "waiting-for-confirmation");
    Ok(json!({
        "state": state_name,
        "attemptId": state.get("attemptId").cloned().unwrap_or(Value::Null),
        "ownerLeaseId": state.get("ownerLeaseId").cloned().unwrap_or(Value::Null),
        "qrImageDataUri": if active { state.get("qrImageDataUri").cloned().unwrap_or(json!("")) } else { json!("") },
        "launchUrl": if active { state.get("launchUrl").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "expiresAtMs": state.get("expiresAtMs").cloned().unwrap_or(json!(0)),
        "pollAfterMs": 1500,
        "profile": Value::Null,
        "entitlement": Value::Null,
        "providerId": provider_id(platform),
        "profileId": profile,
        "revision": now_ms(),
        "capabilities": capabilities(platform, false)
    })
    .to_string())
}

fn capabilities(platform: &str, authenticated: bool) -> Value {
    json!({
        "qrLogin": platform != "spotify",
        "favoriteRead": authenticated && platform != "kugou",
        "favoriteWrite": false,
        "playlistRead": authenticated,
        "playlistWrite": false,
        "recentHistoryRead": false
    })
}

fn guest_snapshot(
    platform: &str,
    profile: &str,
    state: &str,
    attempt_id: Option<Value>,
) -> Result<String, String> {
    Ok(json!({
        "state": state,
        "attemptId": attempt_id.unwrap_or(Value::Null),
        "profile": Value::Null,
        "entitlement": Value::Null,
        "providerId": provider_id(platform),
        "profileId": profile,
        "revision": now_ms(),
        "capabilities": capabilities(platform, false)
    })
    .to_string())
}

fn authenticated_snapshot(
    platform: &str,
    profile: &str,
    display_name: Option<String>,
) -> Result<String, String> {
    Ok(authenticated_snapshot_value(platform, profile, display_name, now_ms()).to_string())
}

fn authenticated_snapshot_value(
    platform: &str,
    profile: &str,
    display_name: Option<String>,
    revision: u64,
) -> Value {
    json!({
        "state": "authenticated",
        "profile": {
            "avatarUrl": Value::Null,
            "nickname": display_name.unwrap_or_else(|| platform.to_owned()),
            "maskedIdentity": "authenticated"
        },
        "entitlement": unknown_entitlement(),
        "providerId": provider_id(platform),
        "profileId": profile,
        "revision": revision,
        "capabilities": capabilities(platform, true)
    })
}

fn unknown_entitlement() -> Value {
    json!({
        "tier": "unknown", "membership": "unknown", "expiresAtMs": Value::Null,
        "permittedQualities": [], "observedMaximumQuality": Value::Null, "restrictions": []
    })
}

fn snapshot(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload).to_owned();
    let qr_key = state_key(platform, &profile, "qr-attempt");
    if let Some(raw) = storage_get(&qr_key)? {
        let state: Value = serde_json::from_str(&raw)
            .map_err(|_| error("storage-failure", "QR login state is invalid", false))?;
        if json_u64(&state, &["expiresAtMs"]).unwrap_or(0) > now_ms() {
            return qr_snapshot_from_state(platform, &profile, &state);
        }
        storage_delete(&qr_key)?;
    }
    let result = match platform {
        "spotify" => {
            if credential_handle(&credential_key(platform, &profile, "refresh-token"))?.is_some() {
                spotify_account_snapshot(&profile)
            } else {
                guest_snapshot(platform, &profile, "guest", None)
            }
        }
        "netease" => {
            if credential_handle(&credential_key(platform, &profile, "cookie"))?.is_some() {
                netease_account_snapshot(&profile)
            } else {
                guest_snapshot(platform, &profile, "guest", None)
            }
        }
        "kugou" => {
            if credential_handle(&credential_key(platform, &profile, "cookie-api"))?.is_some() {
                kugou_account_snapshot(&profile)
            } else {
                guest_snapshot(platform, &profile, "guest", None)
            }
        }
        _ => Err(unsupported("account.snapshot")),
    };
    session_snapshot_result(platform, &profile, result)
}

fn session_snapshot_result(
    platform: &str,
    profile: &str,
    result: Result<String, String>,
) -> Result<String, String> {
    match result {
        Err(reason)
            if serde_json::from_str::<Value>(&reason)
                .ok()
                .and_then(|value| json_string(&value, &["code"]))
                .as_deref()
                == Some("authentication-expired") =>
        {
            guest_snapshot(platform, profile, "session-expired", None)
        }
        result => result,
    }
}

fn spotify_account_snapshot(profile: &str) -> Result<String, String> {
    let access = spotify_access(profile)?;
    let response = request(
        "GET",
        &format!("{SPOTIFY_API}/v1/me"),
        None,
        None,
        Some((access.0.as_str(), "authorization", "Bearer ")),
        None,
    )?;
    authenticated_snapshot(
        "spotify",
        profile,
        account_display_name("spotify", &response)?,
    )
}

fn netease_account_snapshot(profile: &str) -> Result<String, String> {
    let response = netease_request(profile, "/api/w/nuser/account/get", json!({}))?;
    netease_catalog::account(profile, &response)
}

fn kugou_account_snapshot(profile: &str) -> Result<String, String> {
    let cookie = auth_header("kugou", profile, "cookie-api", "cookie", "")?;
    let response = kugou_request(
        "POST",
        KUGOU_API,
        "/v3/get_my_info",
        &[("plat", "1".to_owned())],
        cookie.as_ref().map(|value| value.0.as_str()),
        KugouSignature::Android,
        Some("usercenter.kugou.com"),
        false,
    )?;
    kugou_account(profile, &response)
}

fn kugou_avatar(response: &Value) -> Option<String> {
    [
        &["data", "pic"][..],
        &["data", "user_pic"],
        &["data", "avatar"],
        &["data", "base_info", "pic"],
        &["data", "base_info", "user_pic"],
        &["data", "base_info", "avatar"],
    ]
    .iter()
    .filter_map(|path| json_string(response, path))
    .map(|value| normalize_kugou_artwork(&value))
    .find(|value| artwork_request_url("kugou", value).is_some())
}

fn kugou_account(profile: &str, response: &Value) -> Result<String, String> {
    let mut snapshot = authenticated_snapshot_value(
        "kugou",
        profile,
        account_display_name("kugou", response)?,
        now_ms(),
    );
    snapshot["profile"]["avatarUrl"] = json!(kugou_avatar(response));
    Ok(snapshot.to_string())
}

fn account_display_name(platform: &str, response: &Value) -> Result<Option<String>, String> {
    let malformed = || {
        error(
            "malformed-response",
            "the account response has no valid identity",
            true,
        )
    };
    let expired = || {
        error(
            "authentication-expired",
            "the account session is no longer authorized",
            false,
        )
    };
    match platform {
        "spotify" => {
            if response
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| id.trim().is_empty())
            {
                return Err(malformed());
            }
            Ok(json_string(response, &["display_name"]))
        }
        "netease" => {
            require_netease_success(response)?;
            let profile = response
                .get("profile")
                .or_else(|| response.get("data").and_then(|data| data.get("profile")))
                .ok_or_else(malformed)?;
            if profile.is_null() || json_u64(profile, &["userId"]) == Some(0) {
                return Err(expired());
            }
            if json_u64(profile, &["userId"]).is_none_or(|id| id == 0) {
                return Err(malformed());
            }
            Ok(json_string(profile, &["nickname"]))
        }
        "kugou" => {
            if json_u64(response, &["status"]) == Some(0) {
                return Err(expired());
            }
            if json_u64(response, &["status"]) != Some(1)
                || response
                    .get("data")
                    .and_then(Value::as_object)
                    .is_none_or(|data| data.is_empty())
            {
                return Err(malformed());
            }
            // The signed user-info endpoint does not necessarily echo userid.
            // Its successful profile fields belong to the credential-bound request.
            first_string(
                response,
                &[
                    &["data", "nickname"],
                    &["data", "user_name"],
                    &["data", "base_info", "nickname"],
                ],
            )
            .filter(|name| !name.trim().is_empty())
            .map(Some)
            .ok_or_else(malformed)
        }
        _ => Err(unsupported("account.snapshot")),
    }
}

fn sign_out(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    for kind in [
        "cookie",
        "cookie-login",
        "cookie-api",
        "cookie-media",
        "access-token",
        "refresh-token",
    ] {
        clear_credential(&credential_key(platform, profile, kind))?;
    }
    storage_delete(&state_key(platform, profile, "access-expires"))?;
    storage_delete(&state_key(platform, profile, "oauth-attempt"))?;
    storage_delete(&state_key(platform, profile, "qr-attempt"))?;
    guest_snapshot(platform, profile, "guest", None)
}

fn account_profile_is_authenticated(platform: &str, profile: &str) -> Result<(), String> {
    let credential_kind = match platform {
        "spotify" => "refresh-token",
        "netease" => "cookie",
        "kugou" => "cookie-api",
        _ => return Err(unsupported("account")),
    };
    if credential_handle(&credential_key(platform, profile, credential_kind))?.is_none() {
        return Err(error(
            "authentication-expired",
            "account login is required",
            false,
        ));
    }
    Ok(())
}

fn account_cursor(payload: &Value) -> Result<u64, String> {
    match payload.get("cursor") {
        None | Some(Value::Null) => Ok(0),
        Some(Value::String(value)) => value
            .parse::<u64>()
            .ok()
            .filter(|offset| *offset <= 10_000_000)
            .ok_or_else(|| error("invalid-request", "account cursor is invalid", false)),
        _ => Err(error("invalid-request", "account cursor is invalid", false)),
    }
}

fn required_array<'response>(
    response: &'response Value,
    path: &str,
) -> Result<&'response Vec<Value>, String> {
    response
        .pointer(path)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "the account collection is missing",
                true,
            )
        })
}

fn next_account_cursor(offset: u64, consumed: usize, more: bool) -> Result<Option<String>, String> {
    if !more {
        return Ok(None);
    }
    if consumed == 0 {
        return Err(error(
            "malformed-response",
            "the account page made no progress",
            true,
        ));
    }
    Ok(Some(offset.saturating_add(consumed as u64).to_string()))
}

fn spotify_next_cursor(
    response: &Value,
    offset: u64,
    consumed: usize,
) -> Result<Option<String>, String> {
    let more = match response.get("next") {
        Some(Value::Null) => false,
        Some(Value::String(value)) if !value.is_empty() => true,
        _ => {
            return Err(error(
                "malformed-response",
                "Spotify account pagination is missing",
                true,
            ))
        }
    };
    next_account_cursor(offset, consumed, more)
}

fn account_page(items: Vec<Value>, next_cursor: Option<String>, total: Option<u64>) -> String {
    account_page_value(items, next_cursor, total, now_ms()).to_string()
}

fn account_page_value(
    items: Vec<Value>,
    next_cursor: Option<String>,
    total: Option<u64>,
    timestamp: u64,
) -> Value {
    json!({
        "items": items,
        "nextCursor": next_cursor,
        "total": total,
        "fetchedAtMs": timestamp,
        "stale": false,
        "authRevision": timestamp
    })
}

fn mark_favorite(mut song: Value) -> Value {
    if let Some(object) = song.as_object_mut() {
        object.insert("isFavorite".to_owned(), Value::Bool(true));
    }
    song
}

fn playlist_artwork(value: Option<&Value>, title: &str) -> Value {
    let source = value
        .and_then(Value::as_str)
        .map(normalize_kugou_artwork)
        .unwrap_or_default();
    json!({
        "src": source,
        "alt": title,
        "dominantColor": "#334155",
        "variants": []
    })
}

fn playlist_capabilities() -> Value {
    json!({
        "canAddTracks": false,
        "canRemoveTracks": false,
        "canRename": false,
        "canDelete": false,
        "canReorder": false
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Provider DTO projection keeps its wire fields explicit"
)]
fn playlist_summary(
    platform: &str,
    profile: &str,
    id: String,
    title: String,
    description: String,
    owner_id: String,
    owner_name: String,
    artwork: Option<&Value>,
    ownership: &str,
    track_count: u64,
) -> Value {
    let artwork = playlist_artwork(artwork, &title);
    let reference = if ownership == "owned" {
        json!({"kind": "owned", "tid": id})
    } else {
        json!({"kind": "collected", "tid": id})
    };
    json!({
        "providerId": provider_id(platform),
        "profileId": profile,
        "id": id,
        "reference": reference,
        "title": title,
        "description": description,
        "owner": {"id": owner_id, "displayName": owner_name},
        "artwork": artwork,
        "ownership": ownership,
        "capabilities": playlist_capabilities(),
        "trackCount": track_count,
        "updatedAtMs": Value::Null
    })
}

fn spotify_playlist_summary(item: &Value, profile: &str, owner_id: &str) -> Option<Value> {
    let id = item.get("id").and_then(Value::as_str)?.to_owned();
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled playlist")
        .to_owned();
    let owner = item.get("owner").cloned().unwrap_or(Value::Null);
    let item_owner_id = owner
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(owner_id)
        .to_owned();
    let owner_name = owner
        .get("display_name")
        .and_then(Value::as_str)
        .unwrap_or("Spotify")
        .to_owned();
    let artwork = item
        .get("images")
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .and_then(|image| image.get("url"));
    Some(playlist_summary(
        "spotify",
        profile,
        id,
        title,
        item.get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        item_owner_id.clone(),
        owner_name,
        artwork,
        if item_owner_id == owner_id {
            "owned"
        } else {
            "collected"
        },
        json_u64(item, &["items", "total"])
            .or_else(|| json_u64(item, &["tracks", "total"]))
            .unwrap_or(0),
    ))
}

fn netease_playlist_summary(item: &Value, profile: &str, user_id: &str) -> Option<Value> {
    let id = json_id_string(item.get("id"), String::new());
    if id.is_empty() {
        return None;
    }
    let title = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled playlist")
        .to_owned();
    let owner = item.get("creator").cloned().unwrap_or(Value::Null);
    let owner_id = json_id_string(owner.get("userId"), user_id.to_owned());
    Some(playlist_summary(
        "netease",
        profile,
        id,
        title,
        item.get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        owner_id.clone(),
        owner
            .get("nickname")
            .and_then(Value::as_str)
            .unwrap_or("NetEase Cloud Music")
            .to_owned(),
        item.get("coverImgUrl"),
        if owner_id == user_id {
            "owned"
        } else {
            "collected"
        },
        json_u64(item, &["trackCount"]).unwrap_or(0),
    ))
}

fn kugou_playlist_summary(item: &Value, profile: &str) -> Option<Value> {
    let id = json_id_string(item.get("listid"), String::new());
    if !id.parse::<u64>().is_ok_and(|id| id > 0) {
        return None;
    }
    let ownership = if json_u64(item, &["type"]) == Some(1) {
        "collected"
    } else {
        "owned"
    };
    let title = first_string(item, &[&["name"], &["specialname"]])
        .unwrap_or_else(|| "Untitled playlist".to_owned());
    Some(playlist_summary(
        "kugou",
        profile,
        id,
        title,
        first_string(item, &[&["intro"], &["description"]]).unwrap_or_default(),
        json_id_string(
            item.get("list_create_userid")
                .or_else(|| item.get("userid")),
            "kugou".to_owned(),
        ),
        first_string(item, &[&["list_create_username"], &["username"]])
            .unwrap_or_else(|| "KuGou Music".to_owned()),
        item.get("pic")
            .or_else(|| item.get("img"))
            .or_else(|| item.get("cover")),
        ownership,
        json_u64(item, &["song_count"])
            .or_else(|| json_u64(item, &["songcount"]))
            .or_else(|| json_u64(item, &["count"]))
            .unwrap_or(0),
    ))
}

fn spotify_me_id(profile: &str) -> Result<String, String> {
    let access = spotify_access(profile)?;
    let response = request(
        "GET",
        &format!("{SPOTIFY_API}/v1/me"),
        None,
        None,
        Some((access.0.as_str(), "authorization", "Bearer ")),
        None,
    )?;
    response
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "Spotify account identity is missing",
                true,
            )
        })
}

fn netease_account_id(profile: &str) -> Result<String, String> {
    let response = netease_request(profile, "/api/w/nuser/account/get", json!({}))?;
    require_netease_success(&response)?;
    json_u64(&response, &["account", "id"])
        .or_else(|| json_u64(&response, &["profile", "userId"]))
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or_else(|| {
            error(
                "malformed-response",
                "NetEase account identity is missing",
                true,
            )
        })
}

fn spotify_favorite_songs(profile: &str, payload: &Value) -> Result<String, String> {
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let access = spotify_access(profile)?;
    let response = request(
        "GET",
        &query_url(
            SPOTIFY_API,
            "/v1/me/tracks",
            &[
                ("limit", page_limit.to_string()),
                ("offset", offset.to_string()),
            ],
        ),
        None,
        None,
        Some((access.0.as_str(), "authorization", "Bearer ")),
        None,
    )?;
    let raw_items = required_array(&response, "/items")?;
    let items = raw_items
        .iter()
        .filter_map(|item| item.get("track"))
        .filter_map(|item| map_spotify_song(item, profile))
        .map(mark_favorite)
        .collect::<Vec<_>>();
    let next = spotify_next_cursor(&response, offset, raw_items.len())?;
    Ok(account_page(items, next, json_u64(&response, &["total"])))
}

fn netease_favorite_songs(profile: &str, payload: &Value) -> Result<String, String> {
    let user_id = netease_account_id(profile)?;
    let response = netease_request(profile, "/api/song/like/get", json!({"uid": user_id}))?;
    require_netease_success(&response)?;
    let ids = required_array(&response, "/ids")?;
    let offset = account_cursor(payload)? as usize;
    let page_limit = limit(payload) as usize;
    let selected = ids
        .iter()
        .skip(offset)
        .take(page_limit)
        .cloned()
        .collect::<Vec<_>>();
    let songs = if selected.is_empty() {
        Vec::new()
    } else {
        let detail_ids = selected
            .iter()
            .map(|id| json!({"id": id}))
            .collect::<Vec<_>>();
        let detail_payload = serde_json::to_string(&detail_ids).map_err(|_| {
            error(
                "malformed-response",
                "favorite song IDs could not be encoded",
                false,
            )
        })?;
        let detail = netease_request(profile, "/api/v3/song/detail", json!({"c": detail_payload}))?;
        require_netease_success(&detail)?;
        required_array(&detail, "/songs")?
            .iter()
            .filter_map(|item| map_netease_song(item, profile))
            .map(mark_favorite)
            .collect::<Vec<_>>()
    };
    let next = (offset + selected.len() < ids.len()).then(|| (offset + selected.len()).to_string());
    Ok(account_page(songs, next, Some(ids.len() as u64)))
}

fn netease_track_id_page(
    all_tracks: &[Value],
    offset: usize,
    page_limit: usize,
) -> Result<(Vec<Value>, usize), String> {
    let raw_page = all_tracks
        .iter()
        .skip(offset)
        .take(page_limit)
        .collect::<Vec<_>>();
    let selected = raw_page
        .iter()
        .filter_map(|item| item.get("id"))
        .map(|id| json!({"id": id}))
        .collect::<Vec<_>>();
    if !raw_page.is_empty() && selected.is_empty() {
        return Err(error(
            "malformed-response",
            "NetEase playlist track IDs are invalid",
            true,
        ));
    }
    Ok((selected, raw_page.len()))
}

fn favorite_songs(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    account_profile_is_authenticated(platform, profile)?;
    match platform {
        "spotify" => spotify_favorite_songs(profile, payload),
        "netease" => netease_favorite_songs(profile, payload),
        "kugou" => Err(unsupported("account.favorite-songs")),
        _ => Err(unsupported("account.favorite-songs")),
    }
}

fn spotify_playlists(profile: &str, payload: &Value) -> Result<String, String> {
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let owner_id = spotify_me_id(profile)?;
    let access = spotify_access(profile)?;
    let response = request(
        "GET",
        &query_url(
            SPOTIFY_API,
            "/v1/me/playlists",
            &[
                ("limit", page_limit.to_string()),
                ("offset", offset.to_string()),
            ],
        ),
        None,
        None,
        Some((access.0.as_str(), "authorization", "Bearer ")),
        None,
    )?;
    let raw_items = required_array(&response, "/items")?;
    let items = raw_items
        .iter()
        .filter_map(|item| spotify_playlist_summary(item, profile, &owner_id))
        .collect::<Vec<_>>();
    let next = spotify_next_cursor(&response, offset, raw_items.len())?;
    Ok(account_page(items, next, json_u64(&response, &["total"])))
}

fn netease_playlists(profile: &str, payload: &Value) -> Result<String, String> {
    let user_id = netease_account_id(profile)?;
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let response = netease_request(
        profile,
        "/api/user/playlist",
        json!({
            "uid": user_id,
            "limit": page_limit,
            "offset": offset,
            "includeVideo": true
        }),
    )?;
    require_netease_success(&response)?;
    let raw_items = required_array(&response, "/playlist")?;
    let items = raw_items
        .iter()
        .filter_map(|item| netease_playlist_summary(item, profile, &user_id))
        .collect::<Vec<_>>();
    let total = json_u64(&response, &["total"]);
    let more = response
        .get("more")
        .and_then(Value::as_bool)
        .or_else(|| total.map(|total| offset.saturating_add(raw_items.len() as u64) < total))
        .ok_or_else(|| {
            error(
                "malformed-response",
                "NetEase account pagination is missing",
                true,
            )
        })?;
    let next = next_account_cursor(offset, raw_items.len(), more)?;
    Ok(account_page(items, next, total))
}

fn kugou_list_items(response: &Value) -> Result<&[Value], String> {
    if json_u64(response, &["status"]) != Some(1) {
        return Err(error(
            "malformed-response",
            "KuGou collection status is invalid",
            true,
        ));
    }
    response
        .get("data")
        .and_then(|data| {
            data.as_array()
                .or_else(|| data.get("info").and_then(Value::as_array))
                .or_else(|| data.get("lists").and_then(Value::as_array))
                .or_else(|| data.get("list").and_then(Value::as_array))
                .map(Vec::as_slice)
                .or_else(|| {
                    (data.get("count").is_some_and(|value| {
                        value.as_u64() == Some(0) || value.as_str() == Some("0")
                    }) && data.get("info").is_some_and(Value::is_null))
                    .then_some(&[][..])
                })
        })
        .ok_or_else(|| error("malformed-response", "KuGou collection is missing", true))
}

fn kugou_collection_total(response: &Value) -> Option<u64> {
    [
        &["data", "total"][..],
        &["total"],
        &["data", "count"],
        &["data", "list_count"],
    ]
    .iter()
    .find_map(|path| json_u64(response, path))
}

fn kugou_page_number(offset: u64, page_limit: u64) -> Result<u64, String> {
    if offset % page_limit != 0 {
        return Err(error(
            "invalid-request",
            "KuGou cursor requires the same page size",
            false,
        ));
    }
    Ok(offset / page_limit + 1)
}

fn kugou_next_cursor(
    response: &Value,
    offset: u64,
    page_limit: u64,
    consumed: usize,
) -> Result<Option<String>, String> {
    let total = kugou_collection_total(response);
    let more = total
        .map(|total| offset.saturating_add(page_limit) < total)
        .unwrap_or(consumed as u64 >= page_limit);
    if more && consumed == 0 {
        return Err(error(
            "malformed-response",
            "KuGou account page made no progress",
            true,
        ));
    }
    Ok(more.then(|| offset.saturating_add(page_limit).to_string()))
}

fn kugou_playlists(profile: &str, payload: &Value) -> Result<String, String> {
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let page = kugou_page_number(offset, page_limit)?;
    let response = kugou_request_body(
        profile,
        "/v7/get_all_list",
        &json!({
            "total_ver": 979,
            "type": 2,
            "page": page,
            "pagesize": page_limit
        })
        .to_string(),
    )?;
    let raw_items = kugou_list_items(&response)?;
    let items = raw_items
        .iter()
        .filter_map(|item| kugou_playlist_summary(item, profile))
        .collect::<Vec<_>>();
    let total = kugou_collection_total(&response);
    let next = kugou_next_cursor(&response, offset, page_limit, raw_items.len())?;
    Ok(account_page(items, next, total))
}

fn account_playlists(platform: &str, payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    account_profile_is_authenticated(platform, profile)?;
    match platform {
        "spotify" => spotify_playlists(profile, payload),
        "netease" => netease_playlists(profile, payload),
        "kugou" => kugou_playlists(profile, payload),
        _ => Err(unsupported("account.playlists")),
    }
}

fn playlist_input(payload: &Value, platform: &str) -> Result<(Value, String), String> {
    let profile = platform_id(payload);
    let playlist = payload
        .get("playlist")
        .cloned()
        .ok_or_else(|| error("invalid-request", "playlist is missing", false))?;
    if playlist.get("providerId").and_then(Value::as_str) != Some(provider_id(platform).as_str())
        || playlist.get("profileId").and_then(Value::as_str) != Some(profile)
    {
        return Err(error("invalid-request", "playlist scope is invalid", false));
    }
    let id = playlist
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 512)
        .ok_or_else(|| error("invalid-request", "playlist ID is invalid", false))?
        .to_owned();
    let valid_id = if platform == "spotify" {
        id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    } else {
        id.parse::<u64>().is_ok_and(|value| value > 0)
    };
    if !valid_id || playlist.pointer("/reference/tid").and_then(Value::as_str) != Some(id.as_str())
    {
        return Err(error(
            "invalid-request",
            "playlist reference is invalid",
            false,
        ));
    }
    Ok((playlist, id))
}

fn spotify_playlist_tracks(
    profile: &str,
    playlist: Value,
    id: &str,
    payload: &Value,
) -> Result<String, String> {
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let access = spotify_access(profile)?;
    let response = request(
        "GET",
        &query_url(
            SPOTIFY_API,
            &format!("/v1/playlists/{id}/items"),
            &[
                ("limit", page_limit.to_string()),
                ("offset", offset.to_string()),
            ],
        ),
        None,
        None,
        Some((access.0.as_str(), "authorization", "Bearer ")),
        None,
    )?;
    let raw_tracks = required_array(&response, "/items")?;
    let tracks = raw_tracks
        .iter()
        .filter_map(|item| {
            item.get("item")
                .filter(|track| !track.is_null())
                .or_else(|| item.get("track"))
        })
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("track"))
        .filter_map(|item| map_spotify_song(item, profile))
        .collect::<Vec<_>>();
    let next = spotify_next_cursor(&response, offset, raw_tracks.len())?;
    Ok(json!({
        "summary": playlist,
        "tracks": account_page_value(tracks, next, json_u64(&response, &["total"]), now_ms())
    })
    .to_string())
}

#[derive(Deserialize)]
struct NeteasePlaylistIdResponse {
    code: Option<i64>,
    playlist: Option<NeteasePlaylistIds>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NeteasePlaylistIds {
    track_ids: Option<Vec<CompactSearchSong>>,
}

#[derive(Deserialize)]
struct NeteaseSongDetailResponse {
    code: Option<i64>,
    songs: Option<Vec<CompactSearchSong>>,
}

fn netease_playlist_tracks(
    profile: &str,
    playlist: Value,
    id: &str,
    payload: &Value,
) -> Result<String, String> {
    let offset = account_cursor(payload)? as usize;
    let page_limit = limit(payload) as usize;
    // Only fetch the ID list here. Asking for every song's full metadata
    // exhausts Wasm fuel even when the caller only wants the first 20 tracks.
    let response = netease_response::<NeteasePlaylistIdResponse>(
        profile,
        "/api/v6/playlist/detail",
        json!({"id": id, "n": 0, "s": 0}),
    )?
    .body;
    require_netease_success(&json!({"code": response.code}))?;
    let all_tracks = response
        .playlist
        .and_then(|playlist| playlist.track_ids)
        .ok_or_else(|| {
            error(
                "malformed-response",
                "NetEase playlist track IDs are missing",
                true,
            )
        })?
        .into_iter()
        .map(|item| item.0)
        .collect::<Vec<_>>();
    let (selected, consumed) = netease_track_id_page(&all_tracks, offset, page_limit)?;
    let tracks = if selected.is_empty() {
        Vec::new()
    } else {
        let detail_ids = serde_json::to_string(&selected).map_err(|_| {
            error(
                "malformed-response",
                "playlist track IDs could not be encoded",
                false,
            )
        })?;
        let detail = netease_response::<NeteaseSongDetailResponse>(
            profile,
            "/api/v3/song/detail",
            json!({"c": detail_ids}),
        )?
        .body;
        require_netease_success(&json!({"code": detail.code}))?;
        detail
            .songs
            .as_ref()
            .ok_or_else(|| {
                error(
                    "malformed-response",
                    "NetEase playlist songs are missing",
                    true,
                )
            })?
            .iter()
            .filter_map(|item| map_netease_song(&item.0, profile))
            .collect::<Vec<_>>()
    };
    let next = (offset + consumed < all_tracks.len()).then(|| (offset + consumed).to_string());
    Ok(json!({
        "summary": playlist,
        "tracks": account_page_value(tracks, next, Some(all_tracks.len() as u64), now_ms())
    })
    .to_string())
}

fn kugou_playlist_tracks(
    profile: &str,
    playlist: Value,
    id: &str,
    payload: &Value,
) -> Result<String, String> {
    let offset = account_cursor(payload)?;
    let page_limit = limit(payload);
    let page = kugou_page_number(offset, page_limit)?;
    let response = kugou_request_body(
        profile,
        "/v4/get_list_all_file_v3",
        &json!({
                "listid": id,
                "type": if playlist.get("ownership").and_then(Value::as_str) == Some("collected") {1} else {0},
                "page": page,
                "pagesize": page_limit,
                "area_code": 1,
                "allplatform": 1,
                "show_cover": 1
            })
            .to_string(),
    )?;
    let raw_tracks = kugou_list_items(&response)?;
    let tracks = raw_tracks
        .iter()
        .filter_map(|item| map_kugou_song(item, profile))
        .collect::<Vec<_>>();
    let total = kugou_collection_total(&response);
    let next = kugou_next_cursor(&response, offset, page_limit, raw_tracks.len())?;
    Ok(json!({
        "summary": playlist,
        "tracks": account_page_value(tracks, next, total, now_ms())
    })
    .to_string())
}

fn playlist_tracks(platform: &str, payload: &Value) -> Result<String, String> {
    let (playlist, id) = playlist_input(payload, platform)?;
    let profile = platform_id(payload);
    account_profile_is_authenticated(platform, profile)?;
    match platform {
        "spotify" => spotify_playlist_tracks(profile, playlist, &id, payload),
        "netease" => netease_playlist_tracks(profile, playlist, &id, payload),
        "kugou" => kugou_playlist_tracks(profile, playlist, &id, payload),
        _ => Err(unsupported("account.playlist-tracks")),
    }
}

fn song_detail(platform: &str, payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    if id.is_empty() {
        return Err(error("invalid-request", "track ID is empty", false));
    }
    let profile = platform_id(payload);
    let item = match platform {
        "spotify" => {
            let access = spotify_access(profile)?;
            request(
                "GET",
                &format!("{SPOTIFY_API}/v1/tracks/{id}"),
                None,
                None,
                Some((access.0.as_str(), "authorization", "Bearer ")),
                None,
            )?
        }
        "netease" => {
            let response = netease_request(
                profile,
                "/api/v3/song/detail",
                json!({"c": format!("[{{\"id\":{id}}}]")}),
            )?;
            require_netease_success(&response)?;
            response
                .get("songs")
                .and_then(Value::as_array)
                .and_then(|songs| songs.first())
                .cloned()
                .ok_or_else(|| error("not-found", "song was not found", false))?
        }
        "kugou" => {
            let url = query_url(
                KUGOU_MOBILE,
                "/app/i/getSongInfo.php",
                &[("cmd", "playInfo".to_owned()), ("hash", id.clone())],
            );
            let response = request_with_meta_headers(
                "GET",
                &url,
                None,
                None,
                None,
                None,
                &[(
                    "user-agent",
                    "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/124.0.0.0 Mobile Safari/537.36"
                        .to_owned(),
                )],
            )?
            .body;
            if response
                .get("errcode")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
            {
                return Err(error("not-found", "song was not found", false));
            }
            response
        }
        _ => return Err(unsupported("catalog.song")),
    };
    let mapped = match platform {
        "spotify" => map_spotify_song(&item, profile),
        "netease" => map_netease_song(&item, profile),
        "kugou" => map_kugou_song(&item, profile),
        _ => None,
    }
    .ok_or_else(|| {
        error(
            "malformed-response",
            "the song response is incomplete",
            true,
        )
    })?;
    Ok(mapped.to_string())
}

fn lyrics(platform: &str, payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    let profile = platform_id(payload);
    if id.is_empty() {
        return Err(error("invalid-request", "track ID is empty", false));
    }
    if platform == "netease" {
        return netease_catalog::lyrics(profile, &id);
    }
    let (plain, synced) = match platform {
        "spotify" => return Err(unsupported("lyrics.get")),
        "netease" => {
            let response = netease_request(
                profile,
                "/api/song/lyric",
                json!({"id": id, "tv": -1, "lv": -1, "rv": -1, "kv": -1, "_nmclfl": 1}),
            )?;
            require_netease_success(&response)?;
            let plain = json_string(&response, &["lrc", "lyric"]);
            let synced = plain.as_deref().map(parse_lrc).unwrap_or_default();
            (plain, synced)
        }
        "kugou" => {
            let detail = parse_object(&song_detail(platform, payload)?)?;
            let title = json_string(&detail, &["title"]).unwrap_or_default();
            let duration_ms = json_u64(&detail, &["durationMs"]).unwrap_or(0);
            let search = kugou_request(
                "GET",
                KUGOU_LYRICS,
                "/search",
                &[
                    ("ver", "1".to_owned()),
                    ("client", "pc".to_owned()),
                    ("hash", id.clone()),
                    (
                        "album_audio_id",
                        json_u64(&detail, &["provider", "numericId"])
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "0".to_owned()),
                    ),
                    ("duration", duration_ms.to_string()),
                    ("keyword", title.clone()),
                    ("lrctxt", "1".to_owned()),
                    ("man", "no".to_owned()),
                ],
                None,
                KugouSignature::Android,
                None,
                false,
            )?;
            let Some((lyric_id, access_key)) = kugou_lyric_candidate(&search, &title, duration_ms)
            else {
                return Ok("null".to_owned());
            };
            let response = kugou_request(
                "GET",
                KUGOU_LYRICS,
                "/download",
                &[
                    ("ver", "1".to_owned()),
                    ("client", "android".to_owned()),
                    ("id", lyric_id),
                    ("accesskey", access_key),
                    ("fmt", "lrc".to_owned()),
                    ("charset", "utf8".to_owned()),
                ],
                None,
                KugouSignature::Android,
                None,
                false,
            )?;
            let encoded = json_string(&response, &["content"]).unwrap_or_default();
            let plain = STANDARD
                .decode(encoded.as_bytes())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .or_else(|| (!encoded.is_empty()).then_some(encoded));
            let synced = plain.as_deref().map(parse_lrc).unwrap_or_default();
            (plain, synced)
        }
        _ => return Err(unsupported("lyrics.get")),
    };
    let lines = synced
        .iter()
        .enumerate()
        .map(|(index, line)| {
            json!({
                "id": format!("line-{index}"),
                "startMs": line.0,
                "endMs": Value::Null,
                "text": line.1,
                "words": []
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "songId": id,
        "syncMode": if lines.is_empty() { "unsynchronized" } else { "line" },
        "metadata": {"sourceLabel": platform, "offsetMs": 0},
        "vocalists": [],
        "lines": lines,
        "plain": plain
    })
    .to_string())
}

fn parse_lrc(value: &str) -> Vec<(u64, String)> {
    value
        .lines()
        .filter_map(|line| {
            let close = line.find(']')?;
            let timestamp = line.get(1..close)?;
            let (minutes, seconds) = timestamp.split_once(':')?;
            let minutes = minutes.parse::<u64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            Some((
                minutes
                    .saturating_mul(60_000)
                    .saturating_add((seconds * 1000.0) as u64),
                line.get(close + 1..)?.to_owned(),
            ))
        })
        .collect()
}

fn kugou_lyric_candidate(
    response: &Value,
    title: &str,
    duration_ms: u64,
) -> Option<(String, String)> {
    let title = title
        .rsplit_once(" - ")
        .map_or(title, |(_, song)| song)
        .trim();
    if title.is_empty() || duration_ms == 0 {
        return None;
    }
    response
        .get("candidates")?
        .as_array()?
        .iter()
        .find_map(|item| {
            let candidate_title = json_string(item, &["song"])?;
            let candidate_duration = json_u64(item, &["duration"])?;
            if !candidate_title.trim().eq_ignore_ascii_case(title)
                || duration_ms.abs_diff(candidate_duration) > 2_000
            {
                return None;
            }
            let id = json_string(item, &["id"])?;
            let access_key = json_string(item, &["accesskey"])?;
            if id.is_empty() || access_key.is_empty() {
                None
            } else {
                Some((id, access_key))
            }
        })
}

fn playback(platform: &str, payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    let profile = platform_id(payload);
    if id.is_empty() {
        return Err(error("invalid-request", "track ID is empty", false));
    }
    let quality = playback_quality(platform, payload, &id)?;
    let (url, length, mime, preview, resolved, rights_fallback) = match platform {
        "spotify" => {
            let access = spotify_access(profile)?;
            let response = request(
                "GET",
                &format!("{SPOTIFY_API}/v1/tracks/{id}"),
                None,
                None,
                Some((access.0.as_str(), "authorization", "Bearer ")),
                None,
            )?;
            let preview_url = json_string(&response, &["preview_url"]).ok_or_else(|| {
                error(
                    "unsupported-operation",
                    "Spotify only exposed no preview URL",
                    false,
                )
            })?;
            let (length, mime) = media_metadata(&preview_url)?;
            return Ok(playback_value(
                platform,
                &id,
                &quality,
                "standard",
                &preview_url,
                length,
                &mime,
                true,
                false,
            ));
        }
        "netease" => {
            let response = netease_request(
                profile,
                "/api/song/enhance/player/url/v1",
                json!({"ids": json!([id]).to_string(), "level": netease_quality(&quality), "encodeType": "flac"}),
            )?;
            require_netease_playback_success(&response)?;
            let item = response
                .get("data")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .ok_or_else(|| {
                    error(
                        "full-playback-unavailable",
                        "NetEase returned no media source",
                        false,
                    )
                })?;
            // Diagnose an empty source with the same account/profile. A failed
            // secondary lookup keeps the original, unconfirmed source failure.
            let detail = if json_string(item, &["url"]).is_none_or(|url| url.is_empty()) {
                netease_request(
                    profile,
                    "/api/v3/song/detail",
                    json!({"c": format!("[{{\"id\":{id}}}]")}),
                )
                .ok()
            } else {
                None
            };
            let privilege = detail
                .as_ref()
                .filter(|value| json_u64(value, &["code"]) == Some(200))
                .and_then(|value| value.get("privileges"))
                .and_then(Value::as_array)
                .and_then(|values| {
                    values
                        .iter()
                        .find(|value| json_id_string(value.get("id"), String::new()) == id)
                });
            let url = netease_playback_url(item, privilege)?;
            let declared_length = json_u64(item, &["size"]).unwrap_or(0);
            let (length, detected_mime) = if declared_length > 0 {
                (
                    declared_length,
                    audio_mime(json_string(item, &["type"]).as_deref().unwrap_or("mp3")).to_owned(),
                )
            } else {
                media_metadata(&url)?
            };
            let resolved = media_quality(item, &detected_mime);
            let preview = item.get("freeTrialInfo").is_some_and(Value::is_object);
            (url, length, detected_mime, preview, resolved, false)
        }
        "kugou" => {
            let mut selected = None;
            let mut rights_fallback = false;
            let mut failure_code = "full-playback-unavailable";
            for candidate in kugou_quality_candidates(&quality) {
                let response = kugou_song_url(profile, &id, candidate, payload)?;
                if let Some(url) = kugou_media_url(&response) {
                    selected = Some((response, url));
                    break;
                }
                failure_code = kugou_playback_failure_code(&response);
                if json_u64(&response, &["status"]) != Some(2) {
                    break;
                }
                rights_fallback = true;
            }
            let (response, url) = selected.ok_or_else(|| {
                error(
                    failure_code,
                    "KuGou returned no permitted media source",
                    false,
                )
            })?;
            let declared_length = json_u64(&response, &["fileSize"])
                .or_else(|| json_u64(&response, &["size"]))
                .unwrap_or(0);
            let (length, mime) = if declared_length > 0 {
                (
                    declared_length,
                    audio_mime(
                        json_string(&response, &["extName"])
                            .as_deref()
                            .unwrap_or("mp3"),
                    )
                    .to_owned(),
                )
            } else {
                media_metadata(&url)?
            };
            let resolved = media_quality(&response, &mime);
            (url, length, mime, false, resolved, rights_fallback)
        }
        _ => return Err(unsupported("playback.resolve")),
    };
    if length == 0 {
        return Err(error(
            "malformed-response",
            "the media length is unknown",
            false,
        ));
    }
    Ok(playback_value(
        platform,
        &id,
        &quality,
        resolved,
        &url,
        length,
        &mime,
        preview,
        rights_fallback,
    ))
}

fn netease_copyright_notice(value: &Value) -> bool {
    ["msg", "message"]
        .iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .any(|text| {
            [
                "无版权",
                "暫無版權",
                "暂无版权",
                "版权限制",
                "版權限制",
                "版权保护",
                "版權保護",
                "地区限制",
                "地區限制",
                "not available in your country",
                "not available in your region",
            ]
            .iter()
            .any(|reason| text.contains(reason))
        })
}

fn require_netease_playback_success(response: &Value) -> Result<(), String> {
    let granted = response
        .pointer("/data/0/url")
        .and_then(Value::as_str)
        .is_some_and(|url| !url.is_empty());
    if !granted
        && !matches!(
            response.get("code").and_then(Value::as_i64),
            Some(301 | 401 | 429)
        )
        && netease_copyright_notice(response)
    {
        return Err(error(
            "copyright-restricted",
            "NetEase reported a licensing restriction",
            false,
        ));
    }
    require_netease_success(response)
}

fn netease_playback_url(item: &Value, privilege: Option<&Value>) -> Result<String, String> {
    if let Some(url) = json_string(item, &["url"]).filter(|url| !url.is_empty()) {
        return normalize_netease_media_url(&url).ok_or_else(|| {
            error(
                "malformed-response",
                "NetEase returned an unsafe media URL",
                false,
            )
        });
    }
    let code = match item.get("code").and_then(Value::as_i64) {
        Some(301 | 401) => "authentication-expired",
        Some(429) => "rate-limited",
        _ if privilege
            .and_then(|value| value.get("toast"))
            .and_then(Value::as_bool)
            == Some(true) =>
        {
            "copyright-restricted"
        }
        _ if netease_copyright_notice(item) => "copyright-restricted",
        // Billing labels describe the missing grant, not the subscription's
        // expiry. Explicit licensing evidence takes priority over billing.
        _ if json_u64(item, &["payed"]) == Some(0) && json_u64(item, &["fee"]) == Some(1) => {
            "membership-required"
        }
        _ if json_u64(item, &["payed"]) == Some(0) && json_u64(item, &["fee"]) == Some(4) => {
            "purchase-required"
        }
        _ => "full-playback-unavailable",
    };
    Err(error(
        code,
        "NetEase returned no permitted media source",
        code == "rate-limited",
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Playback recipe projection keeps its wire fields explicit"
)]
fn playback_value(
    platform: &str,
    id: &str,
    quality: &str,
    resolved: &str,
    url: &str,
    length: u64,
    mime: &str,
    preview: bool,
    rights_fallback: bool,
) -> String {
    let quality_capabilities = ["standard", "high", "lossless", "hi-res", "master"].iter().map(|candidate| {
        let supported = validate_quality(platform, candidate).is_ok();
        json!({
            "quality": candidate,
            "entitlement": if rights_fallback && *candidate == quality { "denied" } else { "unknown" },
            "resource": if *candidate == resolved { "available" } else if supported { "unknown" } else { "unavailable" },
            "client": if supported { "supported" } else { "unsupported" },
            "playable": *candidate == resolved,
        })
    }).collect::<Vec<_>>();
    json!({
        "source": {"kind": "https", "request": {"method": "GET", "url": url}},
        "cacheKey": format!("{platform}:{id}:{quality}"),
        "format": match mime {
            "audio/flac" => "flac",
            "audio/aac" | "audio/mp4" => "aac",
            "audio/wav" => "wav",
            _ => "mp3",
        },
        "mimeType": mime,
        "qualityLabel": resolved,
        "contentLength": length,
        "isPreview": preview,
        "selection": {
            "requestedQuality": quality,
            "resolvedQuality": resolved,
            "fallbackReason": if preview {
                json!("preview-only")
            } else if rights_fallback {
                json!("account-rights")
            } else if quality != "automatic" && quality != resolved {
                json!("source-unavailable")
            } else {
                Value::Null
            },
            "preview": preview,
            "qualityCapabilities": quality_capabilities
        }
    })
    .to_string()
}

fn kugou_quality_candidates(quality: &str) -> &'static [&'static str] {
    match quality {
        "hi-res" => &["hi-res", "lossless", "high", "standard"],
        "lossless" => &["lossless", "high", "standard"],
        "high" => &["high", "standard"],
        _ => &["standard"],
    }
}

fn netease_quality(quality: &str) -> &str {
    match quality {
        "high" => "exhigh",
        "lossless" => "lossless",
        "hi-res" => "hires",
        "master" => "jymaster",
        _ => "standard",
    }
}

fn media_quality(item: &Value, mime: &str) -> &'static str {
    if mime == "audio/flac" {
        let level = first_string(item, &[&["level"], &["quality"]]).unwrap_or_default();
        if level == "jymaster" {
            return "master";
        }
        if matches!(level.as_str(), "hires" | "hi-res")
            || json_u64(item, &["sampleRate"]).is_some_and(|value| value > 48_000)
            || json_u64(item, &["bitDepth"]).is_some_and(|value| value > 16)
        {
            return "hi-res";
        }
        return "lossless";
    }
    let bitrate = json_u64(item, &["br"])
        .or_else(|| json_u64(item, &["bitRate"]))
        .unwrap_or(0);
    if bitrate > 128_000 {
        "high"
    } else {
        "standard"
    }
}

fn normalize_netease_media_url(url: &str) -> Option<String> {
    if url.starts_with("https://") {
        return Some(url.to_owned());
    }
    let remainder = url.strip_prefix("http://")?;
    let host = remainder.split('/').next()?.split(':').next()?;
    if host.ends_with(".music.126.net") {
        Some(format!("https://{remainder}"))
    } else {
        None
    }
}

fn audio_mime(format: &str) -> &str {
    match format {
        "flac" | "audio/flac" => "audio/flac",
        "aac" | "audio/aac" => "audio/aac",
        "m4a" | "mp4" | "audio/mp4" => "audio/mp4",
        "wav" | "audio/wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}

fn kugou_media_url(response: &Value) -> Option<String> {
    let urls = response.get("url")?;
    if let Some(url) = urls.as_str() {
        return normalize_kugou_media_url(url);
    }
    urls.as_array()?
        .iter()
        .find_map(|value| normalize_kugou_media_url(value.as_str()?))
}

fn normalize_kugou_media_url(url: &str) -> Option<String> {
    let remainder = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = remainder.split('/').next()?;
    if [
        "fsandroid.kugou.com",
        "fsandroid.tx.kugou.com",
        "fsmobile.kugou.com",
        "fsmobile.tx.kugou.com",
    ]
    .contains(&host)
    {
        Some(format!("https://{remainder}"))
    } else {
        None
    }
}

fn kugou_song_url(
    profile: &str,
    id: &str,
    quality: &str,
    payload: &Value,
) -> Result<Value, String> {
    let hash = id.to_ascii_lowercase();
    let response = kugou_request(
        "GET",
        KUGOU_MEDIA,
        "/v5/url",
        &[
            ("album_id", kugou_playback_reference(payload, "albumId")),
            ("area_code", "1".to_owned()),
            ("hash", hash.clone()),
            ("ssa_flag", "is_fromtrack".to_owned()),
            ("version", "11430".to_owned()),
            ("page_id", "151369488".to_owned()),
            ("quality", kugou_quality(quality).to_owned()),
            (
                "album_audio_id",
                kugou_playback_reference(payload, "numericId"),
            ),
            ("behavior", "play".to_owned()),
            ("pid", "2".to_owned()),
            ("cmd", "26".to_owned()),
            ("pidversion", "3001".to_owned()),
            ("ppage_id", "463467626,350369493,788954147".to_owned()),
            ("cdnBackup", "1".to_owned()),
            ("module", "".to_owned()),
            ("clientver", "11430".to_owned()),
        ],
        auth_header("kugou", profile, "cookie-media", "cookie", "")?
            .as_ref()
            .map(|value| value.0.as_str()),
        KugouSignature::Android,
        Some("trackercdn.kugou.com"),
        true,
    )?;
    let data = response.get("data").cloned().unwrap_or(response);
    Ok(data)
}

fn kugou_playback_reference(payload: &Value, field: &str) -> String {
    json_u64(payload, &["song", "provider", field])
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .unwrap_or_else(|| "0".to_owned())
}

fn kugou_notice_code(value: &Value) -> Option<&'static str> {
    let notices = ["msg", "message", "error", "error_msg"]
        .iter()
        .filter_map(|field| value.get(*field).and_then(Value::as_str))
        .collect::<Vec<_>>();
    let contains = |words: &[&str]| {
        notices
            .iter()
            .any(|notice| words.iter().any(|word| notice.contains(word)))
    };
    if contains(&[
        "暂无版权",
        "无版权",
        "版权限制",
        "版权保护",
        "地区限制",
        "地域限制",
    ]) {
        Some("copyright-restricted")
    } else if contains(&["登录已过期", "登录失效", "token已过期", "token失效"]) {
        Some("authentication-expired")
    } else if contains(&["请先登录", "需要登录", "未登录"]) {
        Some("authentication-required")
    } else if contains(&["需要付费", "需要会员", "会员专享", "购买后", "付费歌曲"])
    {
        Some("entitlement-insufficient")
    } else if contains(&["参数错误", "参数不合法"]) {
        Some("invalid-request")
    } else if contains(&["请求过于频繁", "访问过于频繁"]) {
        Some("rate-limited")
    } else {
        None
    }
}

fn kugou_playback_failure_code(value: &Value) -> &'static str {
    if let Some(code) = kugou_notice_code(value) {
        return code;
    }
    if json_u64(value, &["status"]) == Some(2) {
        return "entitlement-insufficient";
    }
    let urls = value.get("url");
    if urls.is_some_and(|urls| {
        urls.as_str().is_some_and(|url| !url.is_empty())
            || urls.as_array().is_some_and(|urls| {
                urls.iter()
                    .any(|url| url.as_str().is_some_and(|url| !url.is_empty()))
            })
    }) {
        return "malformed-response";
    }
    "full-playback-unavailable"
}

fn require_kugou_success(value: &Value, path: &str) -> Result<(), String> {
    let lyric_success =
        matches!(path, "/search" | "/download") && json_u64(value, &["status"]) == Some(200);
    if ["error_code", "err_code", "errcode"].iter().any(|field| {
        value
            .get(*field)
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
            .is_some_and(|code| code != 0 && !(lyric_success && code == 200))
    }) || json_u64(value, &["status"]) == Some(0)
    {
        let code = kugou_notice_code(value).unwrap_or(if path == "/v3/get_my_info" {
            "authentication-expired"
        } else {
            "provider-unavailable"
        });
        return Err(error(
            code,
            "KuGou rejected the request",
            matches!(code, "provider-unavailable" | "rate-limited"),
        ));
    }
    Ok(())
}

fn kugou_quality(quality: &str) -> &str {
    match quality {
        "high" => "320",
        "lossless" => "flac",
        "hi-res" | "hires" => "hires",
        _ => "128",
    }
}

#[derive(Clone, Copy)]
enum KugouSignature {
    Android,
    Web,
}

#[expect(
    clippy::too_many_arguments,
    reason = "Protocol signing needs the request fields and origin together"
)]
fn kugou_request(
    method: &str,
    base: &str,
    path: &str,
    params: &[(&str, String)],
    credential_handle: Option<&str>,
    signature_kind: KugouSignature,
    router: Option<&str>,
    add_key: bool,
) -> Result<Value, String> {
    kugou_request_inner(
        method,
        base,
        path,
        params,
        credential_handle,
        None,
        signature_kind,
        router,
        add_key,
    )
}

fn kugou_request_body(profile: &str, path: &str, body: &str) -> Result<Value, String> {
    let handle = credential_handle(&credential_key("kugou", profile, "cookie-api"))?
        .ok_or_else(|| error("authentication-expired", "account login is required", false))?;
    kugou_request_inner(
        "POST",
        KUGOU_API,
        path,
        &[("plat", "1".to_owned())],
        Some(handle.as_str()),
        Some(body),
        KugouSignature::Android,
        Some("cloudlist.service.kugou.com"),
        false,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Protocol signing needs the request fields and origin together"
)]
fn kugou_request_inner(
    method: &str,
    base: &str,
    path: &str,
    params: &[(&str, String)],
    credential_handle: Option<&str>,
    body: Option<&str>,
    signature_kind: KugouSignature,
    router: Option<&str>,
    add_key: bool,
) -> Result<Value, String> {
    let mid_key = state_key("kugou", DEFAULT_PROFILE, "device-mid");
    let mid = match storage_get(&mid_key)? {
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value
        }
        _ => {
            let value = random_device_id()?;
            storage_set(&mid_key, &value)?;
            value
        }
    };
    let client_time = (now_ms() / 1000).to_string();
    let mut all = params
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect::<Vec<_>>();
    add_param(&mut all, "dfid", "-");
    add_param(&mut all, "mid", &mid);
    add_param(&mut all, "uuid", "-");
    add_param(&mut all, "appid", "1005");
    add_param(&mut all, "clientver", "20489");
    add_param(&mut all, "clienttime", &client_time);
    if add_key {
        let hash = all
            .iter()
            .find(|(key, _)| key == "hash")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let appid = all
            .iter()
            .find(|(key, _)| key == "appid")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| "1005".to_owned());
        let userid = all
            .iter()
            .find(|(key, _)| key == "userid")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| "0".to_owned());
        add_param(
            &mut all,
            "key",
            &md5_hex(&format!(
                "{hash}57ae12eb6890223e355ccfcb74edf70d{appid}{mid}{userid}"
            ))
            .to_ascii_lowercase(),
        );
    }
    match signature_kind {
        KugouSignature::Android => {
            let signature = md5_sorted_signature(&all, "OIlwieks28dk2k092lksi2UIkp", false, "")
                .to_ascii_lowercase();
            add_param(&mut all, "signature", &signature);
        }
        KugouSignature::Web => {
            let signature =
                md5_sorted_signature(&all, "NVPh5oo715z5DIWAeQlhMDsWXXQV4hwt", false, "")
                    .to_ascii_lowercase();
            add_param(&mut all, "signature", &signature);
        }
    }
    let url = query_url(
        base,
        path,
        &all.iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect::<Vec<_>>(),
    );
    let credential = credential_handle.map(|handle| (handle, "cookie", ""));
    let mut headers = vec![
        (
            "user-agent",
            "Android15-1070-11083-46-0-DiscoveryDRADProtocol-wifi".to_owned(),
        ),
        ("dfid", "-".to_owned()),
        ("mid", mid),
        ("clienttime", client_time),
        ("kg-rc", "1".to_owned()),
        ("kg-thash", "5d816a0".to_owned()),
        ("kg-rec", "1".to_owned()),
        ("kg-rf", "B9EDA08A64250DEFFBCADDEE00F8F25F".to_owned()),
    ];
    if let Some(router) = router {
        headers.push(("x-router", router.to_owned()));
    }
    let response = request_with_meta_headers(
        method,
        &url,
        body,
        body.map(|_| "application/json"),
        credential,
        None,
        &headers,
    )?;
    require_kugou_success(&response.body, path)?;
    Ok(response.body)
}

fn add_param(params: &mut Vec<(String, String)>, key: &str, value: &str) {
    if !params.iter().any(|(name, _)| name == key) {
        params.push((key.to_owned(), value.to_owned()));
    }
}

fn random_device_id() -> Result<String, String> {
    let bytes = random_bytes(16)?;
    let mut seed = String::with_capacity(32);
    for byte in bytes {
        seed.push(hex(byte >> 4));
        seed.push(hex(byte & 0x0f));
    }
    let digest = md5_hex(&seed);
    BigUint::parse_bytes(digest.as_bytes(), 16)
        .map(|value| value.to_str_radix(10))
        .ok_or_else(|| {
            error(
                "storage-failure",
                "KuGou device identity could not be created",
                true,
            )
        })
}

fn md5_sorted_signature(
    params: &[(String, String)],
    salt: &str,
    values_only: bool,
    data: &str,
) -> String {
    let mut sorted = params.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let mut value = String::new();
    if values_only {
        value.push_str("1014");
        let mut values = sorted
            .iter()
            .map(|(_, item)| item.as_str())
            .collect::<Vec<_>>();
        values.sort_unstable();
        for item in values {
            value.push_str(item);
        }
        value.push_str("1014");
    } else {
        value.push_str(salt);
        for (key, item) in sorted {
            value.push_str(&key);
            value.push('=');
            value.push_str(&item);
        }
        value.push_str(data);
        value.push_str(salt);
    }
    md5_hex(&value)
}

fn md5_hex(value: &str) -> String {
    let digest = Md5::digest(value.as_bytes());
    let mut output = String::with_capacity(32);
    for byte in digest {
        output.push(hex(byte >> 4));
        output.push(hex(byte & 0x0f));
    }
    output
}

fn hex_upper(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        _ => (b'A' + value - 10) as char,
    }
}

fn qr_png_data_uri(text: &str) -> Option<String> {
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let scale = 3_i32;
    let border = 4_i32;
    let size = qr.size();
    let width = (size + border * 2) * scale;
    let width_usize = usize::try_from(width).ok()?;
    let stride = width_usize.saturating_add(7) / 8;
    let mut raw = Vec::with_capacity(width_usize.saturating_mul(stride.saturating_add(1)));
    for y in 0..width {
        raw.push(0);
        let mut row = vec![0xff; stride];
        for x in 0..width {
            let module_x = x / scale - border;
            let module_y = y / scale - border;
            let dark = module_x >= 0
                && module_x < size
                && module_y >= 0
                && module_y < size
                && qr.get_module(module_x, module_y);
            if dark {
                let x = usize::try_from(x).ok()?;
                row[x / 8] &= !(1 << (7 - x % 8));
            }
        }
        raw.extend_from_slice(&row);
    }
    if raw.len() != width_usize.saturating_mul(stride.saturating_add(1)) {
        return None;
    }
    let compressed = zlib_store(&raw);
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&[1, 0, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &compressed);
    png_chunk(&mut png, b"IEND", &[]);
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut output = vec![0x78, 0x01];
    let mut offset = 0;
    while offset < data.len() || offset == 0 {
        let length = data.len().saturating_sub(offset).min(65_535);
        let final_block = offset.saturating_add(length) >= data.len();
        output.push(if final_block { 1 } else { 0 });
        let length = length as u16;
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(&(!length).to_le_bytes());
        output.extend_from_slice(&data[offset..offset + usize::from(length)]);
        offset = offset.saturating_add(usize::from(length));
        if final_block {
            break;
        }
    }
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    output.extend_from_slice(&((b << 16) | a).to_be_bytes());
    output
}

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(kind.len() + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn query_parameter(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        (key == name).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (unhex(bytes[index + 1]), unhex(bytes[index + 2])) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8(output).unwrap_or_default()
}

fn unhex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_platforms_advertise_browser_qq_and_wechat_with_platform_scoped_sessions() {
        for platform in ["netease", "kugou"] {
            let methods: Value = serde_json::from_str(&login_methods(platform)).unwrap();
            for method in ["qq", "wechat"] {
                assert!(methods
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value["id"] == method && value["flow"] == "oauth"));
                let prepared =
                    browser_login_configuration(platform, method, 1_700_000_000_000).unwrap();
                assert!(prepared["url"]
                    .as_str()
                    .unwrap()
                    .starts_with(if platform == "netease" {
                        "https://music.163.com/api/sns/authorize?"
                    } else {
                        "https://openplat-user.kugou.com/"
                    }));
                assert_eq!(
                    prepared["browserSession"]["requiredCookieNames"],
                    json!([if platform == "netease" {
                        "MUSIC_U"
                    } else {
                        "KuGoo"
                    }])
                );
                let callback = prepared["callbackMatcher"]["urlPrefix"].as_str().unwrap();
                assert!(callback.starts_with("https://"));
                assert!(callback.ends_with(if platform == "netease" {
                    if method == "qq" {
                        "/back/qq"
                    } else {
                        "/back/weichat"
                    }
                } else if method == "qq" {
                    "/qq_callback"
                } else {
                    "/wechat_callback"
                }));
            }
        }
        assert!(browser_login_configuration("spotify", "qq", 0).is_err());
        assert!(browser_login_configuration("kugou", "unknown", 0).is_err());
    }

    #[test]
    fn browser_credentials_require_one_handle_for_each_expected_origin() {
        let origins = [(NETEASE_API, "cookie")];
        assert_eq!(
            browser_credential_handles(
                &json!({"browserCredentials":[{"origin":NETEASE_API,"handle":"opaque"}]}),
                &origins
            )
            .unwrap(),
            vec![("cookie".to_owned(), "opaque".to_owned())]
        );
        for values in [
            json!([]),
            json!([{"origin":SPOTIFY_API,"handle":"opaque"}]),
            json!([{"origin":NETEASE_API,"handle":""}]),
            json!([{"origin":NETEASE_API,"handle":"opaque"},{"origin":NETEASE_API,"handle":"other"}]),
        ] {
            assert!(
                browser_credential_handles(&json!({"browserCredentials":values}), &origins)
                    .is_err()
            );
        }
    }

    #[test]
    fn qr_poll_errors_are_reported_instead_of_a_successful_waiting_snapshot() {
        let state =
            json!({"phase":"waiting-for-scan", "attemptId":"fixture", "ownerLeaseId":"lease"});
        for platform in ["netease", "kugou"] {
            for code in ["offline", "rate-limited", "malformed-response"] {
                let reason = error(code, "synthetic polling failure", true);
                assert_eq!(
                    finish_qr_poll(platform, "default", state.clone(), Err(reason.clone())),
                    Err(reason)
                );
            }
        }
    }

    #[test]
    fn kugou_qr_status_must_be_explicit_before_expiring_an_attempt() {
        assert!(matches!(
            parse_kugou_qr_check(&json!({"data":{"status":0}})),
            Ok(QrCheck::Expired)
        ));
        assert!(matches!(
            parse_kugou_qr_check(&json!({"data":{"status":1}})),
            Ok(QrCheck::WaitingScan)
        ));
        assert!(matches!(
            parse_kugou_qr_check(&json!({"data":{"status":2}})),
            Ok(QrCheck::WaitingConfirmation)
        ));
        assert!(matches!(
            parse_kugou_qr_check(&json!({"data":{"status":4,"token":"synthetic","userid":42}})),
            Ok(QrCheck::Authenticated { .. })
        ));
        for response in [
            json!({}),
            json!({"data":null}),
            json!({"data":{"status":99}}),
            json!({"data":{"status":4}}),
        ] {
            let Err(reason) = parse_kugou_qr_check(&response) else {
                panic!("invalid status accepted")
            };
            let reason: Value = serde_json::from_str(&reason).unwrap();
            assert_eq!(reason["code"], "malformed-response");
        }
    }

    #[test]
    fn md5_and_form_encoding_are_stable() {
        assert_eq!(md5_hex("abc"), "900150983CD24FB0D6963F7D28E17F72");
        assert_eq!(form_encode("a b/+"), "a%20b%2F%2B");
    }

    #[test]
    fn netease_eapi_wire_format_matches_upstream_vector() {
        let path = "/api/song/enhance/player/url/v1";
        assert_eq!(
            netease_eapi_url(path).unwrap(),
            "https://interfacepc.music.163.com/eapi/song/enhance/player/url/v1"
        );
        assert!(netease_eapi_url("/song/url").is_err());
        assert_eq!(netease_encrypted_params(path, &json!({"encodeType": "flac", "ids": "[\"42\"]", "level": "standard"})), "FA90B329E9614F79E79598F37DC2EDB487F00D1BC4C9B24CD57E6C318B9073567565D994EF3A89F5356C243F90D8099D485DB260FA577059C3E1CFBFB3B6E9C1017826BD5E3EFACA7B294722C2F63063F84B73DC579C5E1C5B0B45149678B5E3E8D49D2395F045EBAE3E4089DC336083D02BCB2B55C3F81305C758A0F2A63F76CFAF87899A341F508937742BE46AEB6EFD1B0C228DFDCBB15D9439E0694F9258");
    }

    #[test]
    fn login_alone_does_not_claim_membership_or_quality_entitlements() {
        let entitlement = unknown_entitlement();
        assert_eq!(entitlement["tier"], "unknown");
        assert_eq!(entitlement["membership"], "unknown");
        assert_eq!(entitlement["permittedQualities"], json!([]));
        assert!(entitlement["observedMaximumQuality"].is_null());
    }

    #[test]
    fn unsupported_operations_do_not_report_false_success() {
        for platform in ["spotify", "kugou"] {
            for kind in ["artist", "album", "playlist"] {
                let result = search(platform, &json!({"kind": kind, "query": "sample"}));
                let failure: Value = serde_json::from_str(&result.unwrap_err()).unwrap();
                assert_eq!(failure["code"], "unsupported-operation");
            }
        }
    }

    #[test]
    fn netease_http_media_urls_are_upgraded_only_for_official_hosts() {
        assert_eq!(
            normalize_netease_media_url("http://m801.music.126.net/song.mp3?token=x").as_deref(),
            Some("https://m801.music.126.net/song.mp3?token=x")
        );
        assert_eq!(
            normalize_netease_media_url("http://example.com/song.mp3"),
            None
        );
        assert_eq!(
            normalize_netease_media_url("https://m10.music.126.net/song.mp3").as_deref(),
            Some("https://m10.music.126.net/song.mp3")
        );
    }

    #[test]
    fn lrc_and_qr_helpers_keep_expected_wire_shapes() {
        assert_eq!(
            parse_lrc("[00:01.50]first\n[01:02.000]second"),
            vec![(1_500, "first".to_owned()), (62_000, "second".to_owned())]
        );
        let qr = qr_png_data_uri("https://music.example/qr").expect("qr png");
        assert!(qr.starts_with("data:image/png;base64,"));
        assert!(qr.len() < 20_000);
        let png = STANDARD
            .decode(qr.trim_start_matches("data:image/png;base64,"))
            .expect("base64 png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(png[24], 1);
        assert_eq!(png[25], 0);
    }

    #[test]
    fn mapped_song_keeps_platform_and_profile_scope() {
        let song = song_value(
            "kugou",
            "work",
            "hash",
            "Song",
            vec![json!({"id": "artist", "name": "Artist"})],
            "Album",
            json!("https://example.com/art.jpg"),
            1_000,
            Value::Null,
            true,
        );
        assert_eq!(
            song["provider"]["providerId"],
            "org.yaqmc.providers.music.kugou"
        );
        assert_eq!(song["provider"]["profileId"], "work");
        assert_eq!(song["provider"]["trackId"], "hash");
    }

    #[test]
    fn netease_catalog_exposes_billing_without_claiming_account_playback_access() {
        for (fee, tier) in [
            (1, Some("music-vip")),
            (4, Some("purchase")),
            (8, None),
            (0, None),
        ] {
            let source = json!({"id": 42, "name": "fixture", "fee": fee});
            let compact: CompactSearchSong = serde_json::from_str(&source.to_string()).unwrap();
            let song = map_netease_song(&compact.0, "work").unwrap();
            if let Some(tier) = tier {
                assert_eq!(song["availability"]["requiredTier"], tier);
            } else {
                assert_eq!(song["availability"]["status"], "available");
            }
            assert!(song.get("playbackCapability").is_none());
            assert_eq!(song["provider"]["profileId"], "work");
        }
        let compact: CompactSearchSong = serde_json::from_value(json!({
            "id":42, "fee":1, "privilege":{"toast":true, "largeUnused":[1,2,3]}
        }))
        .unwrap();
        let song = map_netease_song(&compact.0, "work").unwrap();
        assert_eq!(song["availability"]["reason"], "copyright-restricted");
        assert!(compact.0.pointer("/privilege/largeUnused").is_none());
    }

    #[test]
    fn netease_playback_failure_keeps_account_and_platform_restrictions_distinct() {
        for (item, privilege, expected) in [
            (
                json!({"code":-110, "url":null, "fee":1, "payed":0}),
                None,
                "membership-required",
            ),
            (
                json!({"code":-110, "url":null, "fee":4, "payed":0}),
                None,
                "purchase-required",
            ),
            (
                json!({"code":-110, "url":null, "fee":1, "payed":0}),
                Some(json!({"toast":true})),
                "copyright-restricted",
            ),
            (
                json!({"code":-110, "url":null, "message":"由于版权保护，您所在的地区暂时无法使用"}),
                None,
                "copyright-restricted",
            ),
            (
                json!({"code":301, "url":null, "fee":1, "payed":0}),
                None,
                "authentication-expired",
            ),
            (json!({"code":429, "url":null}), None, "rate-limited"),
            (
                json!({"code":-110, "url":null, "fee":0}),
                None,
                "full-playback-unavailable",
            ),
            (
                json!({"code":-110, "url":null, "fee":8, "payed":0}),
                None,
                "full-playback-unavailable",
            ),
            (
                json!({"code":-110, "url":null, "fee":1, "payed":1}),
                None,
                "full-playback-unavailable",
            ),
            (
                json!({"code":-110, "url":null, "fee":1}),
                None,
                "full-playback-unavailable",
            ),
            (
                json!({"url":"http://evil.invalid/song.mp3"}),
                None,
                "malformed-response",
            ),
        ] {
            let failure: Value =
                serde_json::from_str(&netease_playback_url(&item, privilege.as_ref()).unwrap_err())
                    .unwrap();
            assert_eq!(failure["code"], expected, "{item}");
        }
        // A granted full/preview source wins over billing labels.
        let granted = json!({"url":"http://m801.music.126.net/preview.mp3", "fee":1, "payed":0, "freeTrialInfo":{"start":0,"end":30}});
        assert_eq!(
            netease_playback_url(&granted, None).unwrap(),
            "https://m801.music.126.net/preview.mp3"
        );
        for response in [
            json!({"code":403,"message":"由于版权保护，您所在的地区暂时无法使用"}),
            json!({"code":200,"data":[{"url":null}],"message":"暂无版权"}),
        ] {
            let failure: Value =
                serde_json::from_str(&require_netease_playback_success(&response).unwrap_err())
                    .unwrap();
            assert_eq!(failure["code"], "copyright-restricted");
        }
        assert!(require_netease_playback_success(&json!({"code":200,"data":[{"url":"https://m801.music.126.net/preview.mp3"}],"message":"版权保护"})).is_ok());
        let expired: Value = serde_json::from_str(
            &require_netease_playback_success(&json!({"code":301,"message":"暂无版权"}))
                .unwrap_err(),
        )
        .unwrap();
        assert_eq!(expired["code"], "authentication-expired");
    }

    #[test]
    fn netease_http_covers_are_normalized_before_search_and_queue_mapping() {
        for album_field in ["al", "album"] {
            let mut source = json!({"id": 5257138, "name": "屋顶"});
            source[album_field] =
                json!({"name": "Album", "picUrl": "http://p1.music.126.net/cover.jpg"});
            let compact: CompactSearchSong = serde_json::from_str(&source.to_string()).unwrap();
            for item in [source, compact.0] {
                let song = map_netease_song(&item, "work").unwrap();
                assert_eq!(song["artwork"]["src"], "https://p1.music.126.net/cover.jpg");
                assert_eq!(song["provider"]["profileId"], "work");
                assert!(
                    artwork_request_url("netease", song["artwork"]["src"].as_str().unwrap())
                        .is_some()
                );
            }
        }
        for url in [
            "http://p1.music.126.net.evil.example/cover.jpg",
            "http://user@p1.music.126.net/cover.jpg",
            "http://p1.music.126.net:8080/cover.jpg",
            "http://p1.music.126.net/cover.jpg#fragment",
            "http://example.com/cover.jpg",
        ] {
            let song =
                map_netease_song(&json!({"id": 42, "album": {"picUrl": url}}), "default").unwrap();
            assert_eq!(song["artwork"]["src"], url);
            assert!(artwork_request_url("netease", url).is_none());
        }
    }

    #[test]
    fn compact_search_parsing_keeps_catalog_fields_and_skips_upstream_metadata() {
        let samples = [
            (
                "spotify",
                json!({
                    "id": "spotify-track", "name": "Song", "duration_ms": 180000,
                    "artists": [{"id": "artist", "name": "Artist"}],
                    "album": {"name": "Album", "images": [{"url": "https://example.com/cover.jpg"}]},
                    "preview_url": "https://example.com/preview.mp3"
                }),
            ),
            (
                "netease",
                json!({
                    "id": 5257138, "name": "屋顶", "dt": 319039,
                    "ar": [{"id": 6452, "name": "周杰伦"}],
                    "al": {"name": "Album", "picUrl": "https://example.com/cover.jpg"}
                }),
            ),
            (
                "kugou",
                json!({
                    "hash": "F09B7FAE3D1A3860C10738EBB33DCBBA", "songname": "Song",
                    "singername": "Artist", "album_name": "Album", "duration": 125,
                    "extra": {"128timelength": 125126},
                    "trans_param": {"union_cover": "https://example.com/{size}/cover.jpg"}
                }),
            ),
        ];
        for (platform, mut source) in samples {
            source["unused_metadata"] = json!({"nested": ["x".repeat(50000), {"value": null}]});
            let compact: CompactSearchSong = serde_json::from_str(&source.to_string()).unwrap();
            assert!(compact.0.get("unused_metadata").is_none());
            let mapper = match platform {
                "spotify" => map_spotify_song,
                "netease" => map_netease_song,
                _ => map_kugou_song,
            };
            assert_eq!(mapper(&compact.0, "work"), mapper(&source, "work"));
        }
    }

    #[test]
    fn typed_search_pages_preserve_null_tracks_and_upstream_pagination() {
        let spotify: SpotifySearchResponse = serde_json::from_str(
            r#"{"tracks":{"items":[null,{"id":"track","name":"Song"}],"next":"https://api.spotify.com/v1/search?offset=20"}}"#,
        ).unwrap();
        let tracks = spotify.tracks.unwrap();
        let items = tracks.items.unwrap();
        assert!(items[0].0.is_null());
        assert_eq!(items[1].0["id"], "track");
        assert!(tracks.next.is_some());

        let netease: NeteaseSearchResponse = serde_json::from_str(
            r#"{"code":200,"result":{"songs":[{"id":42,"name":"Song"}],"songCount":40}}"#,
        )
        .unwrap();
        assert_eq!(netease.code, Some(200));
        assert_eq!(netease.result.unwrap().song_count, Some(40));

        let kugou: KugouSearchResponse = serde_json::from_str(
            r#"{"status":1,"data":{"info":null,"lists":[{"Hash":"track","SongName":"Song"}],"total":31}}"#,
        ).unwrap();
        let data = kugou.data.unwrap();
        assert!(data.info.is_none());
        assert_eq!(data.total, Some(31));
        assert_eq!(data.lists.unwrap()[0].0["Hash"], "track");
        assert!(serde_json::from_str::<NeteaseSearchResponse>(
            r#"{"code":200,"result":{"songs":"invalid"}}"#
        )
        .is_err());
    }

    #[test]
    fn playback_track_id_accepts_the_host_song_payload() {
        for (platform, id) in [
            ("spotify", "spotify-track"),
            ("netease", "5257138"),
            ("kugou", "B3A52A7A958BF0AED0EBFBA2E9A818B7"),
        ] {
            let payload = json!({
                "song": {
                    "id": format!("{platform}::default::{id}"),
                    "provider": {"trackId": id}
                }
            });
            assert_eq!(track_id(&payload), id);
            assert_eq!(track_id(&json!({"song": {"id": id}})), id);
            assert_eq!(track_id(&json!({"trackId": id})), id);
            assert_eq!(track_id(&json!({"songId": id})), id);
            assert_eq!(track_id(&json!({"id": format!("{platform}::{id}")})), id);
        }
        assert_eq!(track_id(&json!({})), "");
    }

    #[test]
    fn kugou_lyrics_use_the_candidate_id_and_access_key() {
        let response = json!({"candidates": [
            {"id": "", "accesskey": "ignored", "song": "晴天", "duration": 269_000},
            {"id": "274944371", "accesskey": "lyric-key", "song": "晴天", "duration": 269_000}
        ]});
        assert_eq!(
            kugou_lyric_candidate(&response, "周杰伦 - 晴天", 269_000),
            Some(("274944371".to_owned(), "lyric-key".to_owned()))
        );
        assert_eq!(
            kugou_lyric_candidate(&json!({"candidates": []}), "晴天", 269_000),
            None
        );
        assert_eq!(kugou_lyric_candidate(&json!({}), "晴天", 269_000), None);
    }

    #[test]
    fn artwork_requests_use_only_platform_image_origins_and_bounded_thumbnails() {
        assert_eq!(
            artwork_request_url("netease", "https://p2.music.126.net/cover.jpg"),
            Some("https://p2.music.126.net/cover.jpg?param=300y300".to_owned())
        );
        assert!(artwork_request_url("spotify", "https://i.scdn.co/image/cover").is_some());
        assert!(artwork_request_url("kugou", "https://imge.kugou.com/cover.jpg").is_some());
        for (platform, url) in [
            ("netease", "http://p2.music.126.net/cover.jpg"),
            ("netease", "https://p2.music.126.net.evil.example/cover.jpg"),
            ("netease", "https://user@p2.music.126.net/cover.jpg"),
            ("kugou", "https://imge.kugou.com:444/cover.jpg"),
            ("spotify", "https://i.scdn.co.evil.example/image/cover"),
            ("spotify", "https://i.scdn.co/image/cover#fragment"),
            ("spotify", "https://127.0.0.1/image/cover"),
            ("kugou", "https://i.scdn.co/image/cover"),
        ] {
            assert!(artwork_request_url(platform, url).is_none());
        }
    }

    #[test]
    fn kugou_lyrics_reject_unrelated_or_unverifiable_candidates() {
        let response = json!({"candidates": [
            {"id": "wrong-song", "accesskey": "key", "song": "The Seasons", "duration": 125_126},
            {"id": "wrong-recording", "accesskey": "key", "song": "Monkeys Spinning Monkeys", "duration": 320_317},
            {"id": "no-duration", "accesskey": "key", "song": "Monkeys Spinning Monkeys"},
            {"id": "match", "accesskey": "key", "song": "monkeys spinning monkeys", "duration": 125_000}
        ]});
        assert_eq!(
            kugou_lyric_candidate(&response, "Monkeys Spinning Monkeys", 125_126),
            Some(("match".to_owned(), "key".to_owned()))
        );
        assert_eq!(kugou_lyric_candidate(&response, "", 125_126), None);
        assert_eq!(
            kugou_lyric_candidate(&response, "Monkeys Spinning Monkeys", 0),
            None
        );
    }

    #[test]
    fn kugou_quality_uses_upstream_values() {
        assert_eq!(kugou_quality("standard"), "128");
        assert_eq!(kugou_quality("automatic"), "128");
        assert_eq!(kugou_quality("high"), "320");
        assert_eq!(kugou_quality("lossless"), "flac");
        assert_eq!(kugou_quality("hi-res"), "hires");
    }

    #[test]
    fn kugou_media_arrays_are_upgraded_only_for_granted_cdns() {
        assert_eq!(
            kugou_media_url(
                &json!({"url": ["http://fsandroid.tx.kugou.com/song.mp3?opaque=value"]})
            ),
            Some("https://fsandroid.tx.kugou.com/song.mp3?opaque=value".to_owned())
        );
        assert_eq!(
            kugou_media_url(&json!({"url": "https://fsmobile.kugou.com/song.mp3"})),
            Some("https://fsmobile.kugou.com/song.mp3".to_owned())
        );
        for url in [
            "http://example.com/song.mp3",
            "https://fsandroid.kugou.com.evil.example/song.mp3",
            "http://fsandroid.kugou.com@evil.example/song.mp3",
        ] {
            assert_eq!(normalize_kugou_media_url(url), None);
        }
        assert_eq!(kugou_media_url(&json!({"url": []})), None);
    }

    #[test]
    fn kugou_mobile_duration_is_seconds_without_millisecond_metadata() {
        let song = map_kugou_song(
            &json!({"hash": "hash", "songName": "Song", "timeLength": 125}),
            "default",
        )
        .expect("song");
        assert_eq!(song["durationMs"], 125_000);
        let song = map_kugou_song(&json!({"hash": "hash", "songName": "Song", "timeLength": 125, "extra": {"128timelength": 125126}}), "default").expect("song");
        assert_eq!(song["durationMs"], 125_126);
    }

    #[test]
    fn playback_format_matches_the_media_mime() {
        let value: Value = serde_json::from_str(&playback_value(
            "netease",
            "42",
            "lossless",
            "lossless",
            "https://m10.music.126.net/song.flac",
            100,
            audio_mime("flac"),
            false,
            false,
        ))
        .expect("playback JSON");
        assert_eq!(value["format"], "flac");
        assert_eq!(value["mimeType"], "audio/flac");
        assert_eq!(audio_mime("mp3"), "audio/mpeg");
    }

    #[test]
    fn quality_preferences_validate_platform_support_and_keep_scope() {
        assert!(validate_quality("netease", "master").is_ok());
        assert!(validate_quality("kugou", "hi-res").is_ok());
        assert!(validate_quality("kugou", "master").is_err());
        assert!(validate_quality("spotify", "high").is_err());
        assert!(validate_quality("spotify", "automatic").is_ok());
        assert!(validate_quality("netease", "bogus").is_err());
        let status = status_value("netease", "work", "lossless");
        assert_eq!(status["profileId"], "work");
        assert_eq!(status["preferredQuality"], "lossless");
        assert_eq!(status["capabilities"]["qualitySelection"], true);
        assert_eq!(netease_quality("hi-res"), "hires");
        assert_eq!(netease_quality("master"), "jymaster");
        assert_eq!(netease_quality("automatic"), "standard");
        assert_ne!(
            state_key("netease", "work", "preferred-quality"),
            state_key("kugou", "work", "preferred-quality")
        );
    }

    #[test]
    fn quality_resolution_uses_resource_metadata_instead_of_request_labels() {
        assert_eq!(
            media_quality(&json!({"br": 128000}), "audio/mpeg"),
            "standard"
        );
        assert_eq!(
            media_quality(&json!({"bitRate": 320000}), "audio/mpeg"),
            "high"
        );
        assert_eq!(
            media_quality(&json!({"level": "lossless"}), "audio/flac"),
            "lossless"
        );
        assert_eq!(
            media_quality(&json!({"level": "hires"}), "audio/flac"),
            "hi-res"
        );
        assert_eq!(
            media_quality(&json!({"sampleRate": 96000, "bitDepth": 24}), "audio/flac"),
            "hi-res"
        );
        assert_eq!(
            media_quality(&json!({"level": "hires", "br": 128000}), "audio/mpeg"),
            "standard"
        );
        let value: Value = serde_json::from_str(&playback_value(
            "netease",
            "42",
            "lossless",
            "standard",
            "https://m10.music.126.net/song.mp3",
            100,
            "audio/mpeg",
            false,
            false,
        ))
        .unwrap();
        assert_eq!(value["selection"]["requestedQuality"], "lossless");
        assert_eq!(value["selection"]["resolvedQuality"], "standard");
        assert_eq!(value["selection"]["fallbackReason"], "source-unavailable");
        let value: Value = serde_json::from_str(&playback_value(
            "spotify",
            "42",
            "automatic",
            "standard",
            "https://p.scdn.co/song.mp3",
            100,
            "audio/mpeg",
            true,
            false,
        ))
        .unwrap();
        assert_eq!(value["selection"]["resolvedQuality"], "standard");
        assert_eq!(value["selection"]["fallbackReason"], "preview-only");
        assert_eq!(kugou_quality_candidates("high"), &["high", "standard"]);
        let value: Value = serde_json::from_str(&playback_value(
            "kugou",
            "hash",
            "high",
            "standard",
            "https://fsandroid.kugou.com/song.mp3",
            100,
            "audio/mpeg",
            false,
            true,
        ))
        .unwrap();
        assert_eq!(value["selection"]["fallbackReason"], "account-rights");
        assert_eq!(
            value["selection"]["qualityCapabilities"][1]["entitlement"],
            "denied"
        );
        assert_eq!(
            value["selection"]["qualityCapabilities"][4]["client"],
            "unsupported"
        );
    }

    #[test]
    fn spotify_tokens_are_validated_before_saving_or_reusing_a_previous_account() {
        let response = json!({"access_token":"synthetic-access", "refresh_token":"synthetic-refresh", "token_type":"Bearer", "expires_in":3600});
        assert_eq!(
            spotify_token_response(&response, true).unwrap(),
            ("synthetic-access", Some("synthetic-refresh"), 3600)
        );
        let mut refreshed = response.clone();
        refreshed.as_object_mut().unwrap().remove("refresh_token");
        assert!(spotify_token_response(&refreshed, true).is_err());
        assert_eq!(
            spotify_token_response(&refreshed, false).unwrap(),
            ("synthetic-access", None, 3600)
        );
        for (field, value) in [
            ("access_token", json!("")),
            ("token_type", json!("Unknown")),
            ("expires_in", json!(0)),
            ("expires_in", json!(86401)),
        ] {
            let mut malformed = response.clone();
            malformed[field] = value;
            assert!(spotify_token_response(&malformed, true).is_err());
        }
    }

    #[test]
    fn spotify_token_endpoint_errors_distinguish_expiry_and_client_configuration() {
        let url = "https://accounts.spotify.com/api/token";
        assert_eq!(
            upstream_error_code(url, 400, r#"{"error":"invalid_grant"}"#),
            "authentication-expired"
        );
        assert_eq!(
            upstream_error_code(url, 400, r#"{"error":"invalid_client"}"#),
            "provider-not-configured"
        );
        assert_eq!(
            upstream_error_code(url, 400, r#"{"error":"access_denied"}"#),
            "authorization-rejected"
        );
        assert_eq!(upstream_error_code(url, 429, ""), "rate-limited");
        assert_eq!(
            upstream_error_code(
                "https://api.spotify.com/v1/me",
                400,
                r#"{"error":"invalid_grant"}"#
            ),
            "offline"
        );
        assert_eq!(upstream_error_code(url, 500, "not-json"), "offline");
    }

    #[test]
    fn session_responses_require_a_verified_account_instead_of_any_http_success() {
        assert_eq!(
            account_display_name("spotify", &json!({"id":"user", "display_name":"Listener"}))
                .unwrap(),
            Some("Listener".to_owned())
        );
        assert!(account_display_name("spotify", &json!({"error": {"status":401}})).is_err());
        assert!(account_display_name("spotify", &json!({"id":" "})).is_err());
        assert_eq!(
            account_display_name(
                "netease",
                &json!({"code":200, "profile":{"userId":42,"nickname":"Listener"}})
            )
            .unwrap(),
            Some("Listener".to_owned())
        );
        assert_eq!(
            account_display_name(
                "netease",
                &json!({"code":200, "data":{"profile":{"userId":"42","nickname":"Listener"}}})
            )
            .unwrap(),
            Some("Listener".to_owned())
        );
        for response in [
            json!({"code":200,"profile":null}),
            json!({"code":200,"profile":{"userId":0}}),
            json!({"code":301}),
        ] {
            let reason: Value =
                serde_json::from_str(&account_display_name("netease", &response).unwrap_err())
                    .unwrap();
            assert_eq!(reason["code"], "authentication-expired");
        }
        assert!(account_display_name("netease", &json!({"code":200,"profile":{}})).is_err());
        assert!(account_display_name("netease", &json!({})).is_err());
        assert_eq!(
            account_display_name(
                "kugou",
                &json!({"status":1,"data":{"userid":42,"nickname":"Listener"}})
            )
            .unwrap(),
            Some("Listener".to_owned())
        );
        for response in [
            json!({}),
            json!({"status":1,"data":{}}),
            json!({"status":2,"data":{"nickname":"Not authorized"}}),
        ] {
            assert!(account_display_name("kugou", &response).is_err());
        }
        let reason: Value =
            serde_json::from_str(&account_display_name("kugou", &json!({"status":0})).unwrap_err())
                .unwrap();
        assert_eq!(reason["code"], "authentication-expired");
    }

    #[test]
    fn netease_success_requires_a_code_and_distinguishes_auth_and_rate_limits() {
        assert!(require_netease_success(&json!({"code":200})).is_ok());
        for (response, expected) in [
            (json!({}), "malformed-response"),
            (json!({"code":301}), "authentication-expired"),
            (json!({"code":403}), "provider-unavailable"),
            (json!({"code":429}), "rate-limited"),
            (json!({"code":502}), "provider-unavailable"),
        ] {
            let reason: Value =
                serde_json::from_str(&require_netease_success(&response).unwrap_err()).unwrap();
            assert_eq!(reason["code"], expected);
        }
    }

    #[test]
    fn netease_numeric_artist_ids_match_the_provider_contract() {
        let song = map_netease_song(
            &json!({
                "id": 42,
                "name": "Song",
                "ar": [{"id": 6452, "name": "Artist"}],
                "al": {"name": "Album", "picUrl": "https://example.com/art.jpg"}
            }),
            "default",
        )
        .expect("mapped song");
        assert_eq!(song["artists"][0]["id"], "6452");
    }

    #[test]
    fn kugou_mobile_song_detail_matches_the_provider_contract() {
        let song = map_kugou_song(
            &json!({
                "hash": "B3A52A7A958BF0AED0EBFBA2E9A818B7",
                "songName": "Song",
                "singerName": "Artist",
                "album_name": "Album",
                "imgUrl": "http://imge.kugou.com/stdmusic/{size}/art.jpg",
                "timeLength": 0,
                "extra": {"128timelength": "231000"}
            }),
            "default",
        )
        .expect("mapped song");
        assert_eq!(song["id"], "B3A52A7A958BF0AED0EBFBA2E9A818B7");
        assert_eq!(song["durationMs"], 231000);
        assert_eq!(
            song["artwork"]["src"],
            "https://imge.kugou.com/stdmusic/400/art.jpg"
        );
        assert!(song["artists"].is_array());
        assert!(song["album"].is_object());
        assert!(song["artwork"].is_object());
        assert!(song["provider"].is_object());
        assert!(song.get("songName").is_none());
    }

    #[test]
    fn kugou_catalog_keeps_native_ids_without_claiming_full_playback() {
        let source = json!({"FileHash":"TRACK", "SongName":"Song", "Duration":269,
            "AlbumID":"966846", "MixSongID":"32100650", "Singers":[{"id":3520,"name":"Artist"}]});
        let compact: CompactSearchSong = serde_json::from_value(source.clone()).unwrap();
        for source in [&source, &compact.0] {
            let mapped = map_kugou_song(source, "work").unwrap();
            assert_eq!(mapped["provider"]["trackId"], "TRACK");
            assert_eq!(mapped["provider"]["numericId"], 32100650);
            assert_eq!(mapped["provider"]["albumId"], "966846");
            assert_eq!(mapped["album"]["id"], "966846");
            assert_eq!(mapped["artists"][0]["id"], "3520");
            assert!(mapped.get("playbackCapability").is_none());
            assert_eq!(
                kugou_playback_reference(&json!({"song":mapped}), "numericId"),
                "32100650"
            );
        }
        assert_eq!(kugou_playback_reference(&json!({}), "numericId"), "0");
        let paid = map_kugou_song(&json!({"hash":"TRACK", "error":"需要付费"}), "work").unwrap();
        assert_eq!(paid["availability"]["status"], "entitlement-required");
        let restricted =
            map_kugou_song(&json!({"hash":"TRACK", "error":"暂无版权"}), "work").unwrap();
        assert_eq!(restricted["availability"]["reason"], "copyright-restricted");
    }

    #[test]
    fn kugou_cloud_tracks_preserve_millisecond_duration() {
        let song = map_kugou_song(
            &json!({"fileinfo":{"hash":"TRACK", "filename":"Artist - Song", "timelen":231000}}),
            "work",
        )
        .unwrap();
        assert_eq!(song["durationMs"], 231000);
        assert_eq!(song["provider"]["profileId"], "work");
    }

    #[test]
    fn kugou_avatars_use_only_the_granted_https_image_origin() {
        assert_eq!(
            kugou_avatar(
                &json!({"data":{"pic":"http://imge.kugou.com/kugouicon/{size}/avatar.jpg"}})
            )
            .as_deref(),
            Some("https://imge.kugou.com/kugouicon/400/avatar.jpg")
        );
        assert_eq!(
            kugou_avatar(
                &json!({"data":{"pic":"", "base_info":{"pic":"https://imge.kugou.com/avatar.jpg"}}})
            )
            .as_deref(),
            Some("https://imge.kugou.com/avatar.jpg")
        );
        for url in [
            "https://imge.kugou.com.evil.invalid/a.jpg",
            "https://user@imge.kugou.com/a.jpg",
            "https://imge.kugou.com:444/a.jpg",
            "data:image/png;base64,A",
        ] {
            assert!(kugou_avatar(&json!({"data":{"pic":url}})).is_none());
        }
        assert_eq!(account_display_name(
            "kugou",
            &json!({"status":1,"data":{"nickname":"Listener","pic":"https://imge.kugou.com/avatar.jpg"}})
        ).unwrap().as_deref(), Some("Listener"));
        assert!(
            account_display_name("kugou", &json!({"status":1,"data":{"unexpected":1}})).is_err()
        );
        assert_eq!(
            account_display_name(
                "kugou",
                &json!({"status":1,"data":{"base_info":{"userid":42,"nickname":"Listener"}}})
            )
            .unwrap()
            .as_deref(),
            Some("Listener")
        );
    }

    #[test]
    fn kugou_qr_credentials_are_validated_before_storage() {
        for data in [
            json!({"token":"", "userid":42}),
            json!({"token":"x;userid=99", "userid":42}),
            json!({"token":"x\n", "userid":42}),
            json!({"token":"synthetic", "userid":0}),
            json!({"token":"synthetic", "userid":"invalid"}),
        ] {
            let mut response = json!({"data":data});
            response["data"]["status"] = json!(4);
            assert!(parse_kugou_qr_check(&response).is_err());
        }
        let response = json!({"data":{"status":4,"token":"synthetic","user_id":42,"pic":"http://imge.kugou.com/avatar.jpg"}});
        match parse_kugou_qr_check(&response).unwrap() {
            QrCheck::Authenticated {
                secret, avatar_url, ..
            } => {
                assert_eq!(secret, "token=synthetic; userid=42");
                assert_eq!(
                    avatar_url.as_deref(),
                    Some("https://imge.kugou.com/avatar.jpg")
                );
            }
            _ => panic!("expected an authenticated QR response"),
        }
    }

    #[test]
    fn kugou_rejections_preserve_the_reason_without_leaking_upstream_text() {
        for (response, expected) in [
            (
                json!({"status":0,"err_code":10002,"msg":"参数错误"}),
                "invalid-request",
            ),
            (json!({"error_code":-1}), "provider-unavailable"),
            (json!({"errcode":"-1"}), "provider-unavailable"),
            (
                json!({"status":0,"error":"暂无版权 token=synthetic"}),
                "copyright-restricted",
            ),
            (
                json!({"status":0,"msg":"需要付费"}),
                "entitlement-insufficient",
            ),
            (
                json!({"status":0,"msg":"请先登录"}),
                "authentication-required",
            ),
            (
                json!({"status":0,"msg":"登录已过期"}),
                "authentication-expired",
            ),
            (json!({"status":0,"msg":"请求过于频繁"}), "rate-limited"),
        ] {
            let reason = require_kugou_success(&response, "/v5/url").unwrap_err();
            let failure: Value = serde_json::from_str(&reason).unwrap();
            assert_eq!(failure["code"], expected);
            assert!(!reason.contains("synthetic"));
        }
        assert!(require_kugou_success(&json!({"status":1,"err_code":0}), "/v5/url").is_ok());
        for path in ["/search", "/download"] {
            assert!(require_kugou_success(
                &json!({"status":200,"errcode":200,"errmsg":"OK"}),
                path
            )
            .is_ok());
            assert!(require_kugou_success(&json!({"status":0,"errcode":200}), path).is_err());
        }
        assert!(require_kugou_success(&json!({"status":1,"errcode":200}), "/v5/url").is_err());
        assert_eq!(
            kugou_playback_failure_code(&json!({"status":2})),
            "entitlement-insufficient"
        );
        assert_eq!(
            kugou_playback_failure_code(&json!({"status":2,"message":"版权限制"})),
            "copyright-restricted"
        );
        assert_eq!(
            kugou_playback_failure_code(&json!({"url":"https://evil.invalid/song.mp3"})),
            "malformed-response"
        );
        assert_eq!(
            kugou_playback_failure_code(&json!({"url":[]})),
            "full-playback-unavailable"
        );
    }

    #[test]
    fn account_playlist_mappers_preserve_platform_scope_and_numeric_ids() {
        let spotify = spotify_playlist_summary(
            &json!({
                "id": "playlist123",
                "name": "Saved set",
                "description": "description",
                "owner": {"id": "listener", "display_name": "Listener"},
                "images": [{"url": "https://example.com/art.jpg"}],
                "items": {"total": 4}
            }),
            "work",
            "listener",
        )
        .expect("Spotify playlist");
        assert_eq!(spotify["id"], "playlist123");
        assert_eq!(spotify["ownership"], "owned");
        assert_eq!(spotify["trackCount"], 4);
        assert_eq!(spotify["profileId"], "work");

        let netease = netease_playlist_summary(
            &json!({
                "id": 42,
                "name": "Cloud set",
                "creator": {"userId": 7, "nickname": "Listener"},
                "trackCount": "3",
                "coverImgUrl": "https://example.com/cover.jpg"
            }),
            "work",
            "7",
        )
        .expect("NetEase playlist");
        assert_eq!(netease["id"], "42");
        assert_eq!(netease["ownership"], "owned");
        assert_eq!(netease["trackCount"], 3);

        let kugou = kugou_playlist_summary(
            &json!({
                "global_collection_id": "99",
                "listid": "11",
                "specialid": "88",
                "name": "Collected set",
                "type": 1,
                "list_create_userid": 123,
                "list_create_username": "Owner",
                "song_count": "2",
                "img": "http://imge.kugou.com/stdmusic/{size}/cover.jpg"
            }),
            "work",
        )
        .expect("KuGou playlist");
        assert_eq!(kugou["id"], "11");
        assert_eq!(kugou["reference"]["tid"], "11");
        assert_eq!(kugou["ownership"], "collected");
        assert_eq!(kugou["owner"]["id"], "123");
        assert_eq!(kugou["trackCount"], 2);
        assert_eq!(
            kugou["artwork"]["src"],
            "https://imge.kugou.com/stdmusic/400/cover.jpg"
        );
        assert_eq!(kugou["profileId"], "work");
    }

    #[test]
    fn account_pagination_advances_by_raw_items_and_rejects_no_progress() {
        assert_eq!(
            next_account_cursor(0, 2, true).unwrap(),
            Some("2".to_owned())
        );
        assert!(next_account_cursor(0, 0, true).is_err());
        assert_eq!(
            kugou_next_cursor(&json!({"data": {"total": 5}}), 0, 2, 2).unwrap(),
            Some("2".to_owned())
        );
        assert_eq!(
            kugou_next_cursor(&json!({"data": {"total": 5}}), 4, 2, 1).unwrap(),
            None
        );
        assert!(kugou_page_number(1, 20).is_err());

        let (selected, consumed) =
            netease_track_id_page(&[json!({"id": 1}), Value::Null, json!({"id": 3})], 0, 2)
                .expect("NetEase page");
        assert_eq!(consumed, 2);
        assert_eq!(selected, vec![json!({"id": 1})]);
        assert!(netease_track_id_page(&[Value::Null], 0, 1).is_err());
    }

    #[test]
    fn playlist_decoding_preserves_track_pages_without_unused_metadata() {
        let response: NeteasePlaylistIdResponse = serde_json::from_value(json!({
            "code": 200,
            "playlist": {
                "trackIds": [{"id": 1, "unused": [1, 2]}, null, {"id": 3}],
                "tracks": [{"unused": "full metadata"}],
                "creator": {"unused": "profile data"}
            }
        }))
        .unwrap();
        let ids = response
            .playlist
            .unwrap()
            .track_ids
            .unwrap()
            .into_iter()
            .map(|item| item.0)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![json!({"id": 1}), Value::Null, json!({"id": 3})]);
        assert_eq!(
            netease_track_id_page(&ids, 0, 2).unwrap(),
            (vec![json!({"id": 1})], 2)
        );
        assert_eq!(
            netease_track_id_page(&ids, 2, 2).unwrap(),
            (vec![json!({"id": 3})], 1)
        );
        let song = json!({"id": 1, "name": "fixture", "dt": 180000, "ar": [{"id": 2, "name": "artist"}], "al": {"id": 3, "name": "album", "picUrl": "https://p1.music.126.net/cover.jpg"}, "unused": [1, 2, 3]});
        let detail: NeteaseSongDetailResponse =
            serde_json::from_value(json!({"code": 200, "songs": [song.clone()]})).unwrap();
        let compact = &detail.songs.unwrap()[0].0;
        assert!(compact.get("unused").is_none());
        assert_eq!(
            map_netease_song(compact, "work"),
            map_netease_song(&song, "work")
        );
    }

    #[test]
    fn account_collection_shapes_and_playlist_scope_are_strict() {
        let empty = json!({"status":1,"data":{"count":0,"info":null}});
        assert!(kugou_list_items(&empty).unwrap().is_empty());
        assert_eq!(kugou_collection_total(&empty), Some(0));
        assert_eq!(kugou_next_cursor(&empty, 0, 20, 0).unwrap(), None);
        assert!(kugou_list_items(&json!({"status":1,"data":{"count":1,"info":null}})).is_err());
        assert!(kugou_list_items(&json!({"status":1,"data":{"count":-1,"info":null}})).is_err());
        assert_eq!(
            kugou_collection_total(&json!({"data":{"list_count":2}})),
            Some(2)
        );
        let playlist = kugou_playlist_summary(
            &json!({"listid":7,"global_collection_id":"99","name":"Set","count":9}),
            "default",
        )
        .unwrap();
        assert_eq!(playlist["trackCount"], 9);
        assert_eq!(playlist["id"], "7");
        assert!(kugou_list_items(&json!({"status": 1, "data": {}})).is_err());
        assert!(kugou_list_items(&json!({"status": 0, "data": []})).is_err());
        assert!(required_array(&json!({}), "/items").is_err());

        let valid = json!({
            "profileId": "work",
            "playlist": {
                "providerId": "org.yaqmc.providers.music.spotify",
                "profileId": "work",
                "id": "playlist123",
                "reference": {"kind": "owned", "tid": "playlist123"}
            }
        });
        assert_eq!(playlist_input(&valid, "spotify").unwrap().1, "playlist123");
        for invalid in [
            json!({
                "profileId": "work",
                "playlist": {
                    "providerId": "org.yaqmc.providers.music.spotify",
                    "profileId": "work",
                    "id": "../private",
                    "reference": {"kind": "owned", "tid": "../private"}
                }
            }),
            json!({
                "profileId": "other",
                "playlist": {
                    "providerId": "org.yaqmc.providers.music.spotify",
                    "profileId": "work",
                    "id": "playlist123",
                    "reference": {"kind": "owned", "tid": "playlist123"}
                }
            }),
            json!({
                "profileId": "work",
                "playlist": {
                    "providerId": "org.yaqmc.providers.music.spotify",
                    "profileId": "work",
                    "id": "playlist123",
                    "reference": {"kind": "owned", "tid": "different"}
                }
            }),
        ] {
            assert!(playlist_input(&invalid, "spotify").is_err());
        }
    }
}
