//! Bounded, read-only comments. Reply pages are sorted only after the complete
//! upstream thread has been read; a truncated thread is never labelled newest.
use super::*;
use alloc::collections::BTreeMap;
use serde::Serialize;

const MAX_REPLIES: usize = 500;
const REPLY_PAGE_SIZE: u64 = 50;
const MAX_REPLY_PAGES: usize = MAX_REPLIES / REPLY_PAGE_SIZE as usize;
const MAX_COMMENT_OFFSET: u64 = 10_000;

fn invalid_cursor() -> String {
    error(
        "invalid-request",
        "comment cursor does not match the request",
        false,
    )
}

fn limited() -> String {
    error(
        "comments-limit-exceeded",
        "the complete reply thread exceeds the supported sorting limit",
        false,
    )
}

fn malformed() -> String {
    error(
        "malformed-response",
        "NetEase returned an invalid comment page",
        false,
    )
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct ReplyKey {
    time: u64,
    id: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    version: u8,
    scope: Scope,
    offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upstream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before: Option<ReplyKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upper: Option<ReplyKey>,
}

impl Cursor {
    fn parse(raw: Option<&str>, scope: Scope) -> Result<Self, String> {
        let Some(raw) = raw else {
            return Ok(Self {
                version: 1,
                scope,
                offset: 0,
                upstream: None,
                before: None,
                upper: None,
            });
        };
        if raw.len() > 1024 {
            return Err(invalid_cursor());
        }
        let cursor: Self = serde_json::from_str(raw).map_err(|_| invalid_cursor())?;
        if cursor.version != 1
            || cursor.scope != scope
            || cursor.offset > MAX_COMMENT_OFFSET
            || (scope.parent_id.is_some() && (cursor.before.is_none() || cursor.upper.is_none()))
            || (scope.parent_id.is_none() && (cursor.before.is_some() || cursor.upper.is_some()))
        {
            return Err(invalid_cursor());
        }
        Ok(cursor)
    }

    fn encode(&self) -> Result<String, String> {
        let value = serde_json::to_string(self).map_err(|_| malformed())?;
        if value.len() > 1024 {
            return Err(invalid_cursor());
        }
        Ok(value)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentItem {
    comment_id: u64,
    content: Option<String>,
    #[serde(default)]
    liked_count: u64,
    user: Option<CommentUser>,
    time: Option<u64>,
    reply_count: Option<u64>,
    show_floor_comment: Option<CommentFloor>,
    be_replied: Option<Vec<CommentQuote>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentFloor {
    reply_count: Option<u64>,
}
#[derive(Deserialize)]
struct CommentQuote {
    user: Option<CommentUser>,
    content: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentUser {
    #[serde(default)]
    nickname: String,
    avatar_url: Option<String>,
}

fn comment_value(item: CommentItem) -> Option<Value> {
    let content = item.content?;
    if content.trim().is_empty() || item.comment_id == 0 {
        return None;
    }
    let quote = item.be_replied.unwrap_or_default().into_iter().next().and_then(|quote| {
        Some(json!({"author":quote.user.map(|user|user.nickname.chars().take(100).collect::<String>()).unwrap_or_default(),
            "content":quote.content?.chars().take(4000).collect::<String>()}))
    });
    let user = item.user;
    let avatar = user
        .as_ref()
        .and_then(|user| user.avatar_url.as_deref())
        .map(normalize_netease_artwork)
        .filter(|url| artwork_request_url("netease", url).is_some());
    Some(json!({
        "id":item.comment_id.to_string(), "content":content.chars().take(4000).collect::<String>(),
        "author":user.map(|user|user.nickname.chars().take(100).collect::<String>()).unwrap_or_default(),
        "avatarUrl":avatar,"likedCount":item.liked_count.min(9_007_199_254_740_991),"timeMs":item.time,
        "replyCount":item.reply_count.or(item.show_floor_comment.and_then(|floor|floor.reply_count)),"replyTo":quote
    }))
}

pub(super) fn comments_page(payload: &Value) -> Result<String, String> {
    let id = payload
        .get("songId")
        .and_then(Value::as_str)
        .ok_or_else(invalid_cursor)?;
    let valid_id =
        |id: &str| !id.is_empty() && id.len() <= 32 && id.bytes().all(|byte| byte.is_ascii_digit());
    if !valid_id(id) {
        return Err(invalid_cursor());
    }
    let size = payload.get("limit").and_then(Value::as_u64).unwrap_or(20);
    if !(1..=50).contains(&size) {
        return Err(invalid_cursor());
    }
    let sort = payload.get("sort").and_then(Value::as_str).unwrap_or("hot");
    if !matches!(sort, "hot" | "new") {
        return Err(unsupported("comment sort"));
    }
    let parent = payload
        .get("parentId")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .filter(|id| valid_id(id))
                .ok_or_else(invalid_cursor)
        })
        .transpose()?;
    let scope = Scope {
        song_id: id.to_owned(),
        profile_id: platform_id(payload).to_owned(),
        sort: sort.to_owned(),
        parent_id: parent.map(ToOwned::to_owned),
        limit: size,
    };
    let raw = payload
        .get("cursor")
        .filter(|value| !value.is_null())
        .map(|value| value.as_str().ok_or_else(invalid_cursor))
        .transpose()?;
    let cursor = Cursor::parse(raw, scope)?;
    if parent.is_some() {
        if sort != "new" {
            return Err(unsupported("reply sort"));
        }
        return reply_page(cursor);
    }
    let request = json!({"threadId":format!("R_SO_4_{id}"),"pageNo":cursor.offset/size+1,"pageSize":size,"showInner":true,
        "sortType":if sort=="hot" {2} else {3},"cursor":if sort=="hot" {format!("normalHot#{}",cursor.offset)} else {cursor.upstream.clone().unwrap_or_else(||"0".to_owned())}});
    let response =
        netease_response::<Value>(platform_id(payload), "/api/v2/resource/comments", request)?.body;
    require_netease_success(&response)?;
    normalize_top_page(&response, cursor)
}

fn normalize_top_page(response: &Value, mut cursor: Cursor) -> Result<String, String> {
    let body = response.get("data").unwrap_or(response);
    let raw = body
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    if raw.len() > cursor.scope.limit as usize {
        return Err(malformed());
    }
    let total = body
        .get("totalCount")
        .or_else(|| body.get("total"))
        .and_then(Value::as_u64);
    let more = body
        .get("hasMore")
        .or_else(|| body.get("more"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| total.is_some_and(|total| cursor.offset + (raw.len() as u64) < total));
    let mut ids = BTreeMap::new();
    let mut items = Vec::new();
    for raw in raw {
        let item: CommentItem = serde_json::from_value(raw.clone()).map_err(|_| malformed())?;
        if let Some(value) = comment_value(item) {
            if ids
                .insert(value["id"].as_str().unwrap_or_default().to_owned(), ())
                .is_none()
            {
                items.push(value);
            }
        }
    }
    let next = if more && !raw.is_empty() {
        cursor.offset = cursor
            .offset
            .checked_add(raw.len() as u64)
            .ok_or_else(invalid_cursor)?;
        if cursor.offset > MAX_COMMENT_OFFSET {
            return Err(limited());
        }
        cursor.upstream = body
            .get("cursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                raw.last()?
                    .get("time")?
                    .as_u64()
                    .map(|time| time.to_string())
            });
        if cursor.scope.sort == "new"
            && cursor
                .upstream
                .as_deref()
                .is_none_or(|value| value.is_empty() || value == "0")
        {
            return Err(malformed());
        }
        Some(cursor.encode()?)
    } else {
        None
    };
    Ok(json!({"items":items,"nextCursor":next,"total":total,"supportedSorts":["hot","new"],"repliesSupported":true}).to_string())
}

#[derive(Deserialize)]
struct FloorResponse {
    code: Option<i64>,
    data: Option<FloorPage>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FloorPage {
    comments: Vec<CommentItem>,
    has_more: bool,
    total_count: Option<u64>,
    time: Option<u64>,
}

struct ReplyEntry {
    key: ReplyKey,
    value: Value,
}

fn collect_replies<F>(mut fetch: F) -> Result<Vec<ReplyEntry>, String>
where
    F: FnMut(u64) -> Result<FloorPage, String>,
{
    let mut entries = BTreeMap::new();
    let mut time = 0;
    let mut consumed = 0;
    for _ in 0..MAX_REPLY_PAGES {
        let page = fetch(time)?;
        if page
            .total_count
            .is_some_and(|total| total > MAX_REPLIES as u64)
            || page.comments.len() > REPLY_PAGE_SIZE as usize
        {
            return Err(limited());
        }
        consumed += page.comments.len();
        if consumed > MAX_REPLIES {
            return Err(limited());
        }
        let next = page
            .time
            .or_else(|| page.comments.last()?.time)
            .unwrap_or(0);
        let count = page.comments.len();
        for item in page.comments {
            let time = item.time;
            let id = item.comment_id;
            if let Some(value) = comment_value(item) {
                let key = ReplyKey {
                    time: time.ok_or_else(malformed)?,
                    id,
                };
                entries.insert(key.id, ReplyEntry { key, value });
            }
        }
        if !page.has_more {
            return Ok(entries.into_values().collect());
        }
        if count == 0 || next <= time {
            return Err(malformed());
        }
        time = next;
    }
    Err(limited())
}

fn reply_page(cursor: Cursor) -> Result<String, String> {
    let entries = collect_replies(|time| {
        let response=netease_response::<FloorResponse>(&cursor.scope.profile_id,"/api/resource/comment/floor/get",
            json!({"parentCommentId":cursor.scope.parent_id,"threadId":format!("R_SO_4_{}",cursor.scope.song_id),"time":if time==0 {json!(-1)} else {json!(time)},"limit":REPLY_PAGE_SIZE}))?.body;
        require_netease_success(&json!({"code":response.code}))?;
        response.data.ok_or_else(malformed)
    })?;
    sorted_reply_page(entries, cursor)
}

fn sorted_reply_page(mut entries: Vec<ReplyEntry>, mut cursor: Cursor) -> Result<String, String> {
    entries.sort_unstable_by(|a, b| b.key.cmp(&a.key));
    let total = entries.len();
    let upper = cursor
        .upper
        .or_else(|| entries.first().map(|entry| entry.key));
    let mut matching = entries.into_iter().filter(|entry| {
        upper.is_none_or(|upper| entry.key <= upper)
            && cursor.before.is_none_or(|before| entry.key < before)
    });
    let selected = matching
        .by_ref()
        .take(cursor.scope.limit as usize)
        .collect::<Vec<_>>();
    let more = matching.next().is_some();
    let next = if more {
        cursor.upper = upper;
        cursor.before = selected.last().map(|entry| entry.key);
        Some(cursor.encode()?)
    } else {
        None
    };
    let items = selected
        .into_iter()
        .map(|entry| entry.value)
        .collect::<Vec<_>>();
    Ok(json!({"items":items,"nextCursor":next,"total":total,"supportedSorts":["new"],"repliesSupported":false}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(parent: Option<&str>, limit: u64) -> Scope {
        Scope {
            song_id: "186016".to_owned(),
            profile_id: "work".to_owned(),
            sort: "new".to_owned(),
            parent_id: parent.map(ToOwned::to_owned),
            limit,
        }
    }
    fn code(error: String) -> String {
        serde_json::from_str::<Value>(&error).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn item(id: u64, time: u64) -> CommentItem {
        serde_json::from_value(json!({"commentId":id,"time":time,"content":format!("reply-{id}"),"user":{"nickname":"listener"}})).unwrap()
    }
    fn entries(ids: &[u64]) -> Vec<ReplyEntry> {
        ids.iter()
            .map(|id| ReplyEntry {
                key: ReplyKey { time: *id, id: *id },
                value: comment_value(item(*id, *id)).unwrap(),
            })
            .collect()
    }
    fn page(value: String) -> Value {
        serde_json::from_str(&value).unwrap()
    }

    #[test]
    fn cursors_reject_changes_to_every_request_scope_before_network_access() {
        let original = Cursor::parse(None, scope(None, 20)).unwrap();
        let raw = original.encode().unwrap();
        for field in ["songId", "sort", "profileId", "parentId", "limit"] {
            let mut other = serde_json::to_value(&original.scope).unwrap();
            other[field] = match field {
                "limit" => json!(1),
                "parentId" => json!("42"),
                _ => json!("different"),
            };
            assert_eq!(
                code(
                    Cursor::parse(Some(&raw), serde_json::from_value(other).unwrap())
                        .err()
                        .unwrap()
                ),
                "invalid-request"
            );
        }
        assert!(Cursor::parse(Some("{\"offset\":20,\"time\":0}"), scope(None, 20)).is_err());
        assert!(Cursor::parse(Some(&"x".repeat(1025)), scope(None, 20)).is_err());
        let error = comments_page(
            &json!({"songId":"5257138","profileId":"work","sort":"new","limit":20,"cursor":raw}),
        )
        .unwrap_err();
        assert_eq!(code(error), "invalid-request");
    }

    #[test]
    fn top_pages_preserve_quotes_counts_and_advance_over_deleted_items() {
        let response = json!({"code":200,"total":100,"more":true,"comments":[
            {"commentId":1,"content":"hello","likedCount":42,"time":1234,"user":{"nickname":"Listener","avatarUrl":"http://p1.music.126.net/a.jpg"},"showFloorComment":{"replyCount":3},"beReplied":[{"content":"quoted","user":{"nickname":"Other"}}]},
            {"commentId":2,"content":null,"time":1200}
        ]});
        let value = page(
            normalize_top_page(&response, Cursor::parse(None, scope(None, 20)).unwrap()).unwrap(),
        );
        assert_eq!(value["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["items"][0]["likedCount"], 42);
        assert_eq!(value["items"][0]["replyCount"], 3);
        assert_eq!(value["items"][0]["replyTo"]["content"], "quoted");
        assert_eq!(
            value["items"][0]["avatarUrl"],
            "https://p1.music.126.net/a.jpg"
        );
        let cursor = Cursor::parse(value["nextCursor"].as_str(), scope(None, 20)).unwrap();
        assert_eq!(cursor.offset, 2);
        assert_eq!(cursor.upstream.as_deref(), Some("1200"));
        assert_eq!(value["supportedSorts"], json!(["hot", "new"]));
        assert!(normalize_top_page(&json!({"data":{}}), cursor).is_err());
    }

    #[test]
    fn replies_are_globally_descending_across_pages_not_reversed_per_upstream_page() {
        let mut batches = vec![
            FloorPage {
                comments: vec![item(1, 100), item(2, 200)],
                has_more: true,
                total_count: Some(4),
                time: Some(200),
            },
            FloorPage {
                comments: vec![item(3, 300), item(4, 300)],
                has_more: false,
                total_count: Some(4),
                time: Some(300),
            },
        ]
        .into_iter();
        let mut times = Vec::new();
        let values = collect_replies(|time| {
            times.push(time);
            Ok(batches.next().unwrap())
        })
        .unwrap();
        assert_eq!(times, vec![0, 200]);
        let first = page(
            sorted_reply_page(values, Cursor::parse(None, scope(Some("42"), 2)).unwrap()).unwrap(),
        );
        assert_eq!(first["items"][0]["id"], "4");
        assert_eq!(first["items"][1]["id"], "3");
        assert_eq!(first["supportedSorts"], json!(["new"]));
        let cursor = Cursor::parse(first["nextCursor"].as_str(), scope(Some("42"), 2)).unwrap();
        let remaining = vec![
            ReplyEntry {
                key: ReplyKey { time: 100, id: 1 },
                value: comment_value(item(1, 100)).unwrap(),
            },
            ReplyEntry {
                key: ReplyKey { time: 200, id: 2 },
                value: comment_value(item(2, 200)).unwrap(),
            },
            ReplyEntry {
                key: ReplyKey { time: 300, id: 3 },
                value: comment_value(item(3, 300)).unwrap(),
            },
        ];
        let second = page(sorted_reply_page(remaining, cursor).unwrap());
        assert_eq!(second["items"][0]["id"], "2");
        assert_eq!(second["items"][1]["id"], "1");
        assert!(second["nextCursor"].is_null());
    }

    #[test]
    fn reply_cursor_ignores_newer_insertions_and_does_not_skip_after_deletion() {
        let first = page(
            sorted_reply_page(
                entries(&[1, 2, 3, 4, 5]),
                Cursor::parse(None, scope(Some("42"), 2)).unwrap(),
            )
            .unwrap(),
        );
        let cursor = Cursor::parse(first["nextCursor"].as_str(), scope(Some("42"), 2)).unwrap();
        let second = page(sorted_reply_page(entries(&[1, 2, 3, 5, 9]), cursor).unwrap());
        assert_eq!(second["items"][0]["id"], "3");
        assert_eq!(second["items"][1]["id"], "2");
    }

    #[test]
    fn incomplete_or_oversized_threads_never_return_a_partial_newest_page() {
        let error = collect_replies(|_| {
            Ok(FloorPage {
                comments: vec![],
                has_more: true,
                total_count: Some(501),
                time: Some(1),
            })
        })
        .err()
        .unwrap();
        assert_eq!(code(error), "comments-limit-exceeded");
        let mut calls = 0;
        let error = collect_replies(|time| {
            calls += 1;
            Ok(FloorPage {
                comments: vec![item(time + 1, time + 1)],
                has_more: true,
                total_count: None,
                time: Some(time + 1),
            })
        })
        .err()
        .unwrap();
        assert_eq!(calls, MAX_REPLY_PAGES);
        assert_eq!(code(error), "comments-limit-exceeded");
        assert!(collect_replies(|_| Ok(FloorPage {
            comments: vec![],
            has_more: true,
            total_count: None,
            time: Some(0)
        }))
        .is_err());
    }

    #[test]
    fn complete_thread_limit_is_inclusive_and_empty_threads_are_valid() {
        let result = collect_replies(|time| {
            let end = time + REPLY_PAGE_SIZE;
            Ok(FloorPage {
                comments: ((time + 1)..=end).map(|id| item(id, id)).collect(),
                has_more: end < MAX_REPLIES as u64,
                total_count: Some(MAX_REPLIES as u64),
                time: Some(end),
            })
        })
        .unwrap();
        assert_eq!(result.len(), MAX_REPLIES);
        let empty = collect_replies(|_| {
            Ok(FloorPage {
                comments: vec![],
                has_more: false,
                total_count: Some(0),
                time: None,
            })
        })
        .unwrap();
        assert!(empty.is_empty());
        assert_eq!(
            code(
                comments_page(&json!({"songId":"186016","sort":"hot","parentId":"42","limit":20}))
                    .unwrap_err()
            ),
            "unsupported-operation"
        );
    }
}
