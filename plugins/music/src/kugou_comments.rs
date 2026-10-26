//! Public, read-only song comments. Use the service's hot/new endpoints rather
//! than treating its recommended feed or hot replies as chronological results.
use super::*;
use alloc::collections::BTreeSet;
use serde::Serialize;

const CODE: &str = "fc4be23b4e972707f36b8a828a93ba8a";
const SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn invalid() -> String {
    error(
        "invalid-request",
        "comment request or cursor scope is invalid",
        false,
    )
}
fn malformed() -> String {
    error(
        "malformed-response",
        "KuGou returned an invalid comment page",
        true,
    )
}
fn positive(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|id| *id > 0)
}
fn hash(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Scope {
    song_id: String,
    profile_id: String,
    sort: String,
    parent_id: Option<String>,
    limit: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    version: u8,
    scope: Scope,
    page: u32,
    mix_id: u64,
    children_id: u64,
    previous: Option<String>,
}
struct Parent {
    children_id: u64,
    id: u64,
}
impl Parent {
    fn parse(raw: &str, song: &str) -> Result<Self, String> {
        let mut parts = raw.split(':');
        if parts.next() != Some(song) {
            return Err(invalid());
        }
        let children_id = parts
            .next()
            .and_then(|id| id.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or_else(invalid)?;
        let id = parts
            .next()
            .and_then(|id| id.parse::<u64>().ok())
            .filter(|id| *id > 0)
            .ok_or_else(invalid)?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(Self { children_id, id })
    }
}
impl Cursor {
    fn parse(payload: &Value) -> Result<(Self, Option<Parent>), String> {
        let song = payload
            .get("songId")
            .and_then(Value::as_str)
            .filter(|value| hash(value))
            .ok_or_else(invalid)?
            .to_ascii_lowercase();
        let sort = payload.get("sort").and_then(Value::as_str).unwrap_or("hot");
        if !matches!(sort, "hot" | "new") {
            return Err(unsupported("comment sort"));
        }
        let limit = payload
            .get("limit")
            .map(|value| value.as_u64().ok_or_else(invalid))
            .transpose()?
            .unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return Err(invalid());
        }
        let parent_id = payload
            .get("parentId")
            .filter(|value| !value.is_null())
            .map(|value| value.as_str().map(ToOwned::to_owned).ok_or_else(invalid))
            .transpose()?;
        let parent = parent_id
            .as_deref()
            .map(|raw| Parent::parse(raw, &song))
            .transpose()?;
        let scope = Scope {
            song_id: song,
            profile_id: platform_id(payload).to_owned(),
            sort: sort.to_owned(),
            parent_id,
            limit,
        };
        let raw = payload.get("cursor").filter(|value| !value.is_null());
        if let Some(raw) = raw {
            let raw = raw
                .as_str()
                .filter(|raw| raw.len() <= 1024)
                .ok_or_else(invalid)?;
            let cursor: Self = serde_json::from_str(raw).map_err(|_| invalid())?;
            if cursor.version != 1
                || cursor.scope != scope
                || cursor.page < 2
                || cursor.children_id == 0
                || cursor.previous.as_deref().is_some_and(|value| !hash(value))
                || if let Some(parent) = &parent {
                    cursor.mix_id != 0 || cursor.children_id != parent.children_id
                } else {
                    cursor.mix_id == 0
                }
            {
                return Err(invalid());
            }
            Ok((cursor, parent))
        } else {
            Ok((
                Self {
                    version: 1,
                    scope,
                    page: 1,
                    mix_id: 0,
                    children_id: parent.as_ref().map(|p| p.children_id).unwrap_or(0),
                    previous: None,
                },
                parent,
            ))
        }
    }
    fn encode(&self) -> Result<String, String> {
        let value = serde_json::to_string(self).map_err(|_| invalid())?;
        if value.len() > 1024 {
            return Err(invalid());
        }
        Ok(value)
    }
}

// Serde skips badges, images, ads and other unrelated nested metadata before
// allocating it in the component's fixed memory/fuel budget.
#[derive(Deserialize)]
struct CommentPage {
    #[serde(default)]
    status: Value,
    #[serde(default)]
    err_code: Value,
    #[serde(default)]
    error_code: Value,
    #[serde(default)]
    errcode: Value,
    msg: Option<String>,
    #[serde(default)]
    childrenid: Value,
    #[serde(default)]
    tid: Value,
    count: Option<u64>,
    comments_num: Option<u64>,
    current_page: Option<u32>,
    list: Option<Vec<Comment>>,
}
#[derive(Deserialize)]
struct Comment {
    #[serde(default)]
    id: Value,
    content: Option<String>,
    user_name: Option<String>,
    user_pic: Option<String>,
    like: Option<Likes>,
    addtimestamp: Option<u64>,
    addtime: Option<String>,
    reply_num: Option<u64>,
    puser: Option<String>,
    pcontent: Option<String>,
    #[serde(default)]
    special_child_id: Value,
    #[serde(default)]
    tid: Value,
}
#[derive(Deserialize)]
struct Likes {
    count: Option<u64>,
    likenum: Option<u64>,
}

// Legacy reply rows supply Beijing wall-clock time without a numeric timestamp.
// Convert the fixed service format to UTC in O(1), including leap-day validation.
fn service_time(value: &str) -> Option<u64> {
    if value.len() != 19
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || value.as_bytes()[10] != b' '
        || value.as_bytes()[13] != b':'
        || value.as_bytes()[16] != b':'
    {
        return None;
    }
    let number = |start, end| value.get(start..end)?.parse::<u64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > month_days[month as usize - 1] {
        return None;
    }
    let month_start = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][month as usize - 1];
    let previous = year - 1;
    let days = 365 * (year - 1970) + (previous / 4 - 1969 / 4) - (previous / 100 - 1969 / 100)
        + (previous / 400 - 1969 / 400)
        + month_start
        + u64::from(leap && month > 2)
        + day
        - 1;
    (days * 86400 + hour * 3600 + minute * 60 + second)
        .checked_sub(8 * 3600)?
        .checked_mul(1000)
}

fn endpoint(sort: &str, replies: bool) -> &'static str {
    match (sort, replies) {
        ("hot", false) => "/mcomment/r/v1/rank/topliked",
        (_, false) => "/mcomment/r/v1/rank/newest",
        ("hot", true) => "/mcomment/v1/hot_replylist",
        (_, true) => "/mcomment/v1/replylist",
    }
}
fn resolve_mix_id(song: &str, profile: &str) -> Result<u64, String> {
    let detail: Value = serde_json::from_str(&song_detail(
        "kugou",
        &json!({"id":song,"profileId":profile}),
    )?)
    .map_err(|_| malformed())?;
    if detail
        .pointer("/provider/trackId")
        .and_then(Value::as_str)
        .is_none_or(|id| !id.eq_ignore_ascii_case(song))
    {
        return Err(malformed());
    }
    detail
        .pointer("/provider/numericId")
        .and_then(positive)
        .ok_or_else(|| {
            error(
                "not-found",
                "KuGou did not return a comment resource for this song",
                false,
            )
        })
}

pub(super) fn comments_page(payload: &Value) -> Result<String, String> {
    let (mut cursor, parent) = Cursor::parse(payload)?;
    if parent.is_none() && cursor.mix_id == 0 {
        cursor.mix_id = resolve_mix_id(&cursor.scope.song_id, &cursor.scope.profile_id)?;
    }
    let path = endpoint(&cursor.scope.sort, parent.is_some());
    let mut params = vec![
        ("code", CODE.to_owned()),
        ("ver", "6".to_owned()),
        ("p", cursor.page.to_string()),
        ("pagesize", cursor.scope.limit.to_string()),
        ("need_show_image", "0".to_owned()),
        ("extdata", "0".to_owned()),
    ];
    if let Some(parent) = &parent {
        params.push(("childrenid", parent.children_id.to_string()));
        params.push(("tid", parent.id.to_string()));
    } else {
        params.push(("mixsongid", cursor.mix_id.to_string()));
    }
    let page = kugou_request_inner_as::<CommentPage>(
        "POST",
        KUGOU_API,
        path,
        &params,
        None,
        None,
        KugouSignature::Android,
        None,
        false,
    )?;
    normalize(page, cursor, parent.as_ref(), path)
}

fn normalize(
    page: CommentPage,
    mut cursor: Cursor,
    parent: Option<&Parent>,
    path: &str,
) -> Result<String, String> {
    let status = json!({"status":page.status,"err_code":page.err_code,"error_code":page.error_code,"errcode":page.errcode,"msg":page.msg});
    require_kugou_success(&status, path)?;
    if json_u64(&status, &["status"]) != Some(1)
        || page.current_page.is_some_and(|p| p != cursor.page)
    {
        return Err(malformed());
    }
    let children_id = positive(&page.childrenid).ok_or_else(malformed)?;
    if cursor.children_id != 0 && cursor.children_id != children_id {
        return Err(malformed());
    }
    cursor.children_id = children_id;
    if parent.is_some_and(|parent| positive(&page.tid) != Some(parent.id)) {
        return Err(malformed());
    }
    let total = if parent.is_some() {
        page.comments_num
    } else {
        page.count
    };
    let rows = match page.list {
        Some(rows) => rows,
        None if total == Some(0) => Vec::new(),
        _ => return Err(malformed()),
    };
    let raw_count = rows.len() as u64;
    let end = u64::from(cursor.page - 1) * cursor.scope.limit + raw_count;
    if rows.is_empty() && total.is_some_and(|total| total > end) {
        return Err(malformed());
    }
    let mut ids = BTreeSet::new();
    let mut fingerprint = String::new();
    let mut items = Vec::new();
    for item in rows {
        let Some(id) = positive(&item.id) else {
            continue;
        };
        fingerprint.push_str(&format!("{id},"));
        if positive(&item.special_child_id).is_some_and(|id| id != children_id)
            || parent.is_some_and(|parent| positive(&item.tid).is_some_and(|id| id != parent.id))
        {
            return Err(malformed());
        }
        let Some(content) = item.content.filter(|text| !text.trim().is_empty()) else {
            continue;
        };
        if !ids.insert(id) {
            continue;
        }
        let time = item
            .addtimestamp
            .filter(|time| *time > 0 && *time <= 253_402_300_799)
            .map(|time| time * 1000)
            .or_else(|| item.addtime.as_deref().and_then(service_time));
        let avatar = item
            .user_pic
            .as_deref()
            .map(normalize_kugou_artwork)
            .filter(|url| artwork_request_url("kugou", url).is_some());
        let quote = item.pcontent.filter(|text| !text.trim().is_empty()).map(|text|json!({"author":item.puser.unwrap_or_default().chars().take(100).collect::<String>(),"content":text.chars().take(4000).collect::<String>()}));
        let prefix = format!("{}:{children_id}", cursor.scope.song_id);
        let id = if let Some(parent) = parent {
            format!("{prefix}:{}:{id}", parent.id)
        } else {
            format!("{prefix}:{id}")
        };
        let liked = item
            .like
            .and_then(|like| like.count.or(like.likenum))
            .unwrap_or(0)
            .min(SAFE_INTEGER);
        items.push(json!({"id":id,"author":item.user_name.unwrap_or_default().chars().take(100).collect::<String>(),"content":content.chars().take(4000).collect::<String>(),
            "likedCount":liked,"avatarUrl":avatar,"timeMs":time,"replyCount":item.reply_num.map(|count|count.min(SAFE_INTEGER)),"replyTo":quote}));
    }
    let fingerprint = (!fingerprint.is_empty()).then(|| md5_hex(&fingerprint));
    if fingerprint.is_some() && fingerprint == cursor.previous {
        return Err(malformed());
    }
    let more = raw_count > 0
        && total
            .map(|total| end < total)
            .unwrap_or(raw_count >= cursor.scope.limit);
    let next = if more {
        cursor.page = cursor.page.checked_add(1).ok_or_else(invalid)?;
        cursor.previous = fingerprint;
        Some(cursor.encode()?)
    } else {
        None
    };
    Ok(json!({"items":items,"nextCursor":next,"total":total.map(|v|v.min(SAFE_INTEGER)),"supportedSorts":["hot","new"],"repliesSupported":parent.is_none()}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SONG: &str = "768f6fa531a3d883be3f6c91aaf60612";
    fn payload(sort: &str) -> Value {
        json!({"songId":SONG,"profileId":"work","sort":sort,"limit":2})
    }
    fn normalize_value(
        value: Value,
        cursor: Cursor,
        parent: Option<&Parent>,
    ) -> Result<Value, String> {
        let page = serde_json::from_value(value).unwrap();
        serde_json::from_str(&normalize(page, cursor, parent, "test")?).map_err(|_| malformed())
    }
    fn cursor() -> Cursor {
        let mut cursor = Cursor::parse(&payload("new")).unwrap().0;
        cursor.mix_id = 61700953;
        cursor
    }
    fn row(id: u64) -> Value {
        json!({"id":id,"content":"text","user_name":"Listener","user_pic":"http://imge.kugou.com/avatar.jpg","addtime":"2026-10-26 14:56:47","like":{"count":42},"reply_num":725})
    }

    #[test]
    fn converts_service_dates_to_utc_and_rejects_invalid_calendar_values() {
        assert_eq!(service_time("1970-01-01 08:00:00"), Some(0));
        assert_eq!(service_time("2021-03-30 14:10:41"), Some(1_617_084_641_000));
        assert_eq!(service_time("2026-10-26 14:56:47"), Some(1_792_997_807_000));
        assert!(service_time("2000-02-29 12:00:00").is_some());
        for value in [
            "2100-02-29 12:00:00",
            "2025-02-29 12:00:00",
            "2024-13-01 00:00:00",
            "2024-04-31 00:00:00",
            "2024-01-01 24:00:00",
            "2024-01-01 00:00:60",
            "bad",
        ] {
            assert!(service_time(value).is_none(), "{value}");
        }
    }
    #[test]
    fn maps_public_comment_metadata_and_preserves_extra_rows() {
        let mut first = row(1);
        first["like"]["count"] = json!(u64::MAX);
        first["puser"] = json!("Other");
        first["pcontent"] = json!("quoted");
        let page=normalize_value(json!({"status":1,"err_code":0,"childrenid":"9766807","count":10,"current_page":1,"list":[first,row(2),row(3)]}),cursor(),None).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 3);
        assert_eq!(page["items"][0]["id"], format!("{SONG}:9766807:1"));
        assert_eq!(page["items"][0]["likedCount"], SAFE_INTEGER);
        assert_eq!(page["items"][0]["replyCount"], 725);
        assert_eq!(page["items"][0]["replyTo"]["content"], "quoted");
        assert_eq!(
            page["items"][0]["avatarUrl"],
            "https://imge.kugou.com/avatar.jpg"
        );
        let mut request = payload("new");
        request["cursor"] = page["nextCursor"].clone();
        assert_eq!(Cursor::parse(&request).unwrap().0.page, 2);
    }
    #[test]
    fn scopes_cursors_and_parent_references_before_network_access() {
        let page = normalize_value(
            json!({"status":1,"childrenid":"9766807","count":10,"list":[row(1),row(2)]}),
            cursor(),
            None,
        )
        .unwrap();
        let mut valid = payload("new");
        valid["cursor"] = page["nextCursor"].clone();
        assert!(Cursor::parse(&valid).is_ok());
        for (key, value) in [
            ("songId", json!("0123456789abcdef0123456789abcdef")),
            ("profileId", json!("other")),
            ("sort", json!("hot")),
            ("limit", json!(3)),
            ("parentId", json!(format!("{SONG}:9766807:1"))),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(comments_page(&invalid).is_err(), "{key}");
        }
        let mut invalid = payload("new");
        invalid["parentId"] = json!("0123456789abcdef0123456789abcdef:9766807:1");
        assert!(comments_page(&invalid).is_err());
    }
    #[test]
    fn replies_use_native_newest_pagination_without_a_complete_thread_limit() {
        let mut request = payload("new");
        request["parentId"] = json!(format!("{SONG}:9766807:1059924231"));
        let (cursor, parent) = Cursor::parse(&request).unwrap();
        assert_eq!(endpoint("new", true), "/mcomment/v1/replylist");
        assert_ne!(endpoint("hot", true), endpoint("new", true));
        assert_ne!(endpoint("hot", false), endpoint("new", false));
        let mut first = row(633730422);
        first["tid"] = json!(1059924231);
        first["special_child_id"] = json!(9766807);
        let page=normalize_value(json!({"status":1,"childrenid":"9766807","tid":"1059924231","comments_num":725,"list":[first,row(633535316)]}),cursor,parent.as_ref()).unwrap();
        assert_eq!(page["total"], 725);
        assert!(page["nextCursor"].is_string());
        request["cursor"] = page["nextCursor"].clone();
        let (cursor, parent) = Cursor::parse(&request).unwrap();
        let page=normalize_value(json!({"status":1,"childrenid":"9766807","tid":"1059924231","comments_num":725,"current_page":2,"list":[row(776299253),row(632788228)]}),cursor,parent.as_ref()).unwrap();
        assert_eq!(
            page["items"][0]["id"],
            format!("{SONG}:9766807:1059924231:776299253")
        );
    }
    #[test]
    fn validates_empty_pages_source_identity_and_continuation_progress() {
        let empty = normalize_value(
            json!({"status":1,"childrenid":"9766807","count":0,"list":null}),
            cursor(),
            None,
        )
        .unwrap();
        assert!(empty["items"].as_array().unwrap().is_empty());
        assert!(empty["nextCursor"].is_null());
        assert!(
            normalize_value(json!({"childrenid":"9766807","count":0}), cursor(), None).is_err()
        );
        assert!(normalize_value(
            json!({"status":1,"childrenid":"9766807","count":1,"list":null}),
            cursor(),
            None
        )
        .is_err());
        let response = json!({"status":1,"childrenid":"9766807","count":10,"list":[row(1),row(2)]});
        let page = normalize_value(response.clone(), cursor(), None).unwrap();
        let mut request = payload("new");
        request["cursor"] = page["nextCursor"].clone();
        assert!(normalize_value(response, Cursor::parse(&request).unwrap().0, None).is_err());
        let mut wrong = row(3);
        wrong["special_child_id"] = json!(1);
        assert!(normalize_value(
            json!({"status":1,"childrenid":"9766807","count":1,"list":[wrong]}),
            cursor(),
            None
        )
        .is_err());
    }
}
