use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Creator {
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    user_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItem {
    id: u64,
    name: String,
    cover_img_url: Option<String>,
    pic_url: Option<String>,
    description: Option<String>,
    creator: Option<Creator>,
    #[serde(default)]
    track_count: u64,
    tracks: Option<Vec<CompactSearchSong>>,
    track_ids: Option<Vec<CompactSearchSong>>,
}

#[derive(Deserialize)]
struct PlaylistList {
    code: Option<i64>,
    playlists: Option<Vec<PlaylistItem>>,
    list: Option<Vec<PlaylistItem>>,
}

#[derive(Deserialize)]
struct PlaylistDetail {
    code: Option<i64>,
    playlist: Option<PlaylistItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtistItem {
    id: u64,
    name: String,
    pic_url: Option<String>,
    img1v1_url: Option<String>,
    brief_desc: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlbumItem {
    id: u64,
    name: String,
    pic_url: Option<String>,
    artist: ArtistItem,
    publish_time: Option<u64>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct Albums {
    code: Option<i64>,
    albums: Option<Vec<AlbumItem>>,
}

#[derive(Deserialize)]
struct AlbumDetail {
    code: Option<i64>,
    album: Option<AlbumItem>,
    songs: Option<Vec<CompactSearchSong>>,
}

fn malformed() -> String {
    error(
        "malformed-response",
        "NetEase catalog response is incomplete",
        true,
    )
}
fn checked(code: Option<i64>) -> Result<(), String> {
    require_netease_success(&json!({"code": code}))
}
fn art(url: Option<&str>, title: &str) -> Value {
    let url = url.map(normalize_netease_artwork).unwrap_or_default();
    json!({"src": url, "alt": title, "dominantColor": "#334155", "variants": []})
}

impl PlaylistItem {
    fn value(&self, profile: &str) -> Value {
        json!({"id": self.id.to_string(), "title": self.name, "description": self.description.as_deref().unwrap_or(""),
            "owner": {"id": self.creator.as_ref().map(|v| v.user_id.to_string()).unwrap_or_default(), "displayName": self.creator.as_ref().map(|v| v.nickname.as_str()).unwrap_or("网易云音乐")},
            "artwork": art(self.cover_img_url.as_deref().or(self.pic_url.as_deref()), &self.name), "updatedLabel": "",
            "tracks": self.tracks.as_deref().unwrap_or(&[]).iter().filter_map(|v| map_netease_song(&v.0, profile)).collect::<Vec<_>>()})
    }
}

impl AlbumItem {
    fn value(&self, tracks: Vec<Value>) -> Value {
        // Approximate year is only a display hint; use the UTC calendar conversion.
        let year = self.publish_time.map(year_from_ms).unwrap_or(0);
        json!({"id": self.id.to_string(), "title": self.name, "artist": {"id": self.artist.id.to_string(), "name": self.artist.name},
            "artwork": art(self.pic_url.as_deref(), &self.name), "releaseYear": year, "genre": "", "description": self.description.as_deref().unwrap_or(""), "tracks": tracks})
    }
}

fn year_from_ms(ms: u64) -> u64 {
    let mut days = ms / 86_400_000;
    let mut year = 1970;
    while year < 10000 {
        let length = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    year
}

fn list(profile: &str, charts: bool) -> Result<Vec<Value>, String> {
    let response = netease_response::<PlaylistList>(
        profile,
        if charts {
            "/api/toplist"
        } else {
            "/api/playlist/list"
        },
        if charts {
            json!({})
        } else {
            json!({"cat": "全部", "order": "hot", "limit": 8, "offset": 0, "total": true})
        },
    )?
    .body;
    checked(response.code)?;
    Ok(response
        .playlists
        .or(response.list)
        .ok_or_else(malformed)?
        .iter()
        .take(8)
        .map(|v| v.value(profile))
        .collect())
}

pub(super) fn home(payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    let playlists = list(profile, false)?;
    let response = netease_response::<Albums>(profile, "/api/discovery/newAlbum", json!({}))?.body;
    checked(response.code)?;
    let albums = response
        .albums
        .ok_or_else(malformed)?
        .iter()
        .take(8)
        .map(|v| v.value(Vec::new()))
        .collect::<Vec<_>>();
    let featured = albums.first().ok_or_else(malformed)?;
    Ok(json!({"featured": {"eyebrow": "新碟上架", "album": featured}, "recentlyPlayed": [], "madeForYou": [], "newReleases": albums,
        "guessSonglist": null, "recommendedSonglists": playlists, "dailySonglist": null, "newSongSonglist": null, "radarBasedOnSong": null, "radarSongs": []}).to_string())
}

pub(super) fn discover(payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    Ok(json!({"charts": list(profile, true)?, "popularSonglists": list(profile, false)?, "newSongs": null, "newAlbums": [],
        "categories": [], "podcasts": [], "newMvs": [], "featured": []}).to_string())
}

pub(super) fn playlist(payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    let id = track_id(payload);
    let response = netease_response::<PlaylistDetail>(
        profile,
        "/api/v6/playlist/detail",
        json!({"id": id, "n": 0, "s": 0}),
    )?
    .body;
    checked(response.code)?;
    let item = response.playlist.ok_or_else(malformed)?;
    let mut value = item.value(profile);
    let offset = account_cursor(payload)? as usize;
    let all = item.track_ids.as_deref().ok_or_else(malformed)?;
    let ids = all
        .iter()
        .skip(offset)
        .take(20)
        .filter_map(|v| v.0.get("id"))
        .map(|id| json!({"id": id}))
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        let songs = netease_response::<NeteaseSongDetailResponse>(
            profile,
            "/api/v3/song/detail",
            json!({"c": serde_json::to_string(&ids).map_err(|_| malformed())?}),
        )?
        .body;
        checked(songs.code)?;
        value["tracks"] = json!(songs
            .songs
            .ok_or_else(malformed)?
            .iter()
            .filter_map(|v| map_netease_song(&v.0, profile))
            .collect::<Vec<_>>());
    }
    value["nextCursor"] = if offset + 20 < all.len() {
        json!((offset + 20).to_string())
    } else {
        Value::Null
    };
    Ok(value.to_string())
}

pub(super) fn album(payload: &Value) -> Result<String, String> {
    let profile = platform_id(payload);
    let id = track_id(payload);
    if id.parse::<u64>().is_err() {
        return Err(error(
            "invalid-request",
            "NetEase album ID is invalid",
            false,
        ));
    }
    let response = netease_response::<AlbumDetail>(
        profile,
        &format!("/api/v1/album/{id}"),
        json!({"id": id}),
    )?
    .body;
    checked(response.code)?;
    Ok(response
        .album
        .ok_or_else(malformed)?
        .value(
            response
                .songs
                .ok_or_else(malformed)?
                .iter()
                .take(100)
                .filter_map(|v| map_netease_song(&v.0, profile))
                .collect(),
        )
        .to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchItems {
    artists: Option<Vec<ArtistItem>>,
    albums: Option<Vec<AlbumItem>>,
    playlists: Option<Vec<PlaylistItem>>,
    artist_count: Option<u64>,
    album_count: Option<u64>,
    playlist_count: Option<u64>,
}
#[derive(Deserialize)]
struct SearchResponse {
    code: Option<i64>,
    result: Option<SearchItems>,
}

pub(super) fn search(payload: &Value) -> Result<String, String> {
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song");
    let search_type = match kind {
        "artist" => 100,
        "album" => 10,
        "playlist" => 1000,
        _ => return Err(unsupported("catalog.search kind")),
    };
    let query = payload.get("query").and_then(Value::as_str).unwrap_or("");
    let page = page_number(payload);
    let size = limit(payload);
    let response = netease_response::<SearchResponse>(platform_id(payload), "/api/cloudsearch/pc", json!({"s": query, "type": search_type, "limit": size, "offset": (page-1)*size, "total": true}))?.body;
    checked(response.code)?;
    let result = response.result.ok_or_else(malformed)?;
    let (items, total): (Vec<Value>, Option<u64>) = match kind {
        "artist" => (result.artists.unwrap_or_default().iter().map(|v| json!({"id": v.id.to_string(), "name": v.name, "artwork": art(v.pic_url.as_deref().or(v.img1v1_url.as_deref()), &v.name)})).collect(), result.artist_count),
        "album" => (result.albums.unwrap_or_default().iter().map(|v| json!({"id": v.id.to_string(), "title": v.name, "artist": {"id": v.artist.id.to_string(), "name": v.artist.name, "artwork": art(v.artist.pic_url.as_deref(), &v.artist.name)}, "artwork": art(v.pic_url.as_deref(), &v.name), "releaseYear": v.publish_time.map(year_from_ms).unwrap_or(0)})).collect(), result.album_count),
        _ => (result.playlists.unwrap_or_default().iter().map(|v| json!({"id": v.id.to_string(), "title": v.name, "creator": v.creator.as_ref().map(|c| c.nickname.as_str()).unwrap_or(""), "artwork": art(v.cover_img_url.as_deref(), &v.name), "trackCount": v.track_count})).collect(), result.playlist_count),
    };
    Ok(json!({"kind": kind, "query": query, "page": page, "hasMore": total.is_some_and(|v| (page-1)*size+(items.len() as u64)<v), "items": items}).to_string())
}

#[derive(Deserialize)]
struct LyricText {
    lyric: Option<String>,
}
#[derive(Deserialize)]
struct Lyrics {
    code: Option<i64>,
    lrc: Option<LyricText>,
    tlyric: Option<LyricText>,
    romalrc: Option<LyricText>,
}

pub(super) fn lyrics(profile: &str, id: &str) -> Result<String, String> {
    let response = netease_response::<Lyrics>(
        profile,
        "/api/song/lyric",
        json!({"id": id, "tv": -1, "lv": -1, "rv": -1, "kv": -1, "_nmclfl": 1}),
    )?
    .body;
    checked(response.code)?;
    let original = response.lrc.and_then(|v| v.lyric).unwrap_or_default();
    let translated = response
        .tlyric
        .and_then(|v| v.lyric)
        .map(|v| parse_lrc(&v))
        .unwrap_or_default();
    let romanized = response
        .romalrc
        .and_then(|v| v.lyric)
        .map(|v| parse_lrc(&v))
        .unwrap_or_default();
    let mut lines = kugou_lyrics::lrc_lines(&original, 0);
    for line in &mut lines {
        let start = line["startMs"].as_u64().unwrap_or(0);
        line["translation"] = json!(translated
            .iter()
            .find(|v| v.0 == start)
            .map(|v| v.1.as_str()));
        line["romanization"] = json!(romanized
            .iter()
            .find(|v| v.0 == start)
            .map(|v| v.1.as_str()));
    }
    Ok(
        json!({"songId": id, "syncMode": if lines.is_empty(){"unsynchronized"}else{"line"},
        "metadata": {"sourceLabel": "网易云音乐", "offsetMs": 0}, "vocalists": [], "lines": lines})
        .to_string(),
    )
}

pub(super) fn account(profile: &str, response: &Value) -> Result<String, String> {
    let name = account_display_name("netease", response)?;
    let mut snapshot = parse_object(&authenticated_snapshot("netease", profile, name)?)?;
    if let Some(info) = response
        .get("profile")
        .or_else(|| response.pointer("/data/profile"))
    {
        snapshot["profile"]["avatarUrl"] = info
            .get("avatarUrl")
            .and_then(Value::as_str)
            .map(normalize_netease_artwork)
            .filter(|v| v.starts_with("https://"))
            .map(Value::String)
            .unwrap_or(Value::Null);
        if let Some(id) = json_u64(info, &["userId"]) {
            let id = id.to_string();
            let start = id.len().saturating_sub(4);
            snapshot["profile"]["maskedIdentity"] = json!(format!("***{}", &id[start..]));
        }
    }
    if let Ok(vip) = netease_request(
        profile,
        "/api/music-vip-membership/front/vip/info",
        json!({}),
    ) {
        apply_vip(&mut snapshot, &vip, now_ms());
    }
    Ok(snapshot.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtistDetail {
    code: Option<i64>,
    artist: Option<ArtistItem>,
    hot_songs: Option<Vec<CompactSearchSong>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtistAlbums {
    code: Option<i64>,
    hot_albums: Option<Vec<AlbumItem>>,
    more: Option<bool>,
}

fn artist_id(payload: &Value) -> Result<String, String> {
    let id = track_id(payload);
    if id.parse::<u64>().is_err() {
        return Err(error(
            "invalid-request",
            "NetEase artist ID is invalid",
            false,
        ));
    }
    Ok(id)
}

pub(super) fn artist(payload: &Value) -> Result<String, String> {
    let id = artist_id(payload)?;
    let response = netease_response::<ArtistDetail>(
        platform_id(payload),
        &format!("/api/v1/artist/{id}"),
        json!({}),
    )?
    .body;
    checked(response.code)?;
    let info = response.artist.ok_or_else(malformed)?;
    let songs = response
        .hot_songs
        .unwrap_or_default()
        .iter()
        .take(20)
        .filter_map(|v| map_netease_song(&v.0, platform_id(payload)))
        .collect::<Vec<_>>();
    Ok(json!({"id":id,"name":info.name,"artwork":art(info.pic_url.as_deref(),&info.name),"description":info.brief_desc.unwrap_or_default(),"topSongs":songs,"albums":[]}).to_string())
}

pub(super) fn artist_page(payload: &Value) -> Result<String, String> {
    let id = artist_id(payload)?;
    let page = page_number(payload);
    let size = limit(payload);
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("song");
    if kind == "song" {
        let detail = parse_object(&artist(payload)?)?;
        let items = detail["topSongs"]
            .as_array()
            .ok_or_else(malformed)?
            .iter()
            .skip(((page - 1) * size) as usize)
            .take(size as usize)
            .cloned()
            .collect::<Vec<_>>();
        return Ok(
            json!({"kind":kind,"artistId":id,"page":page,"hasMore":false,"items":items})
                .to_string(),
        );
    }
    if kind != "album" {
        return Err(unsupported("catalog.artist-page kind"));
    }
    let response = netease_response::<ArtistAlbums>(
        platform_id(payload),
        &format!("/api/artist/albums/{id}"),
        json!({"limit":size,"offset":(page-1)*size,"total":true}),
    )?
    .body;
    checked(response.code)?;
    let items=response.hot_albums.unwrap_or_default().iter().map(|v|json!({"id":v.id.to_string(),"title":v.name,"artwork":art(v.pic_url.as_deref(),&v.name),"artist":{"id":v.artist.id.to_string(),"name":v.artist.name,"artwork":art(v.artist.pic_url.as_deref(),&v.artist.name)},"releaseYear":v.publish_time.map(year_from_ms).unwrap_or(0)})).collect::<Vec<_>>();
    Ok(json!({"kind":kind,"artistId":id,"page":page,"hasMore":response.more.unwrap_or(false),"items":items}).to_string())
}

fn apply_vip(snapshot: &mut Value, vip: &Value, now: u64) {
    if json_u64(vip, &["code"]) != Some(200) {
        return;
    }
    let Some(data) = vip.get("data") else {
        return;
    };
    let expiry = ["associator", "musicPackage", "redplus"]
        .iter()
        .filter_map(|kind| data.get(*kind))
        .filter_map(|v| json_u64(v, &["expireTime"]))
        .filter(|v| *v > 0)
        .max();
    if let Some(expiry) = expiry {
        snapshot["entitlement"]["tier"] = json!(if expiry > now { "music-vip" } else { "free" });
        snapshot["entitlement"]["membership"] =
            json!(if expiry > now { "active" } else { "expired" });
        snapshot["entitlement"]["expiresAtMs"] = json!(expiry);
    } else if data.get("associator").is_some_and(|v| v.is_object())
        || data.get("musicPackage").is_some_and(|v| v.is_object())
    {
        snapshot["entitlement"]["tier"] = json!("free");
        snapshot["entitlement"]["membership"] = json!("inactive");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn membership_uses_verified_expiry_and_keeps_missing_data_unknown() {
        let mut snapshot = json!({"entitlement":unknown_entitlement()});
        apply_vip(
            &mut snapshot,
            &json!({"code":500,"data":{"associator":{"expireTime":9999}}}),
            1000,
        );
        assert_eq!(snapshot["entitlement"]["membership"], "unknown");
        apply_vip(&mut snapshot, &json!({"code":200,"data":{}}), 1000);
        assert_eq!(snapshot["entitlement"]["tier"], "unknown");
        apply_vip(
            &mut snapshot,
            &json!({"code":200,"data":{"associator":{"expireTime":9999}}}),
            1000,
        );
        assert_eq!(snapshot["entitlement"]["tier"], "music-vip");
        assert_eq!(snapshot["entitlement"]["membership"], "active");
        assert_eq!(snapshot["entitlement"]["expiresAtMs"], 9999);
        assert_eq!(snapshot["entitlement"]["permittedQualities"], json!([]));
        apply_vip(
            &mut snapshot,
            &json!({"code":200,"data":{"associator":{"expireTime":9999}}}),
            10000,
        );
        assert_eq!(snapshot["entitlement"]["membership"], "expired");
    }
    #[test]
    fn catalog_decoding_retains_ids_and_ignores_unused_metadata() {
        let response: PlaylistDetail=serde_json::from_value(json!({"code":200,"playlist":{"id":1,"name":"fixture","trackIds":[{"id":42,"unused":[1,2]}],"trackCount":1,"largeUnused":[]}})).unwrap();
        let item = response.playlist.unwrap();
        assert_eq!(item.value("work")["id"], "1");
        assert_eq!(item.track_ids.unwrap()[0].0, json!({"id":42}));
        assert_eq!(year_from_ms(0), 1970);
        assert_eq!(year_from_ms(1704067200000), 2024);
    }
}
