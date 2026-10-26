# YAQMC integration

## Runtime path

```text
YAQMC UI
  -> provider_* IPC command
  -> YAQMC Core Provider adapter
  -> org.yaqmc.providers.music WebAssembly Component
  -> YAQMC settings/storage/credentials/network imports
  -> platform HTTPS API
```

No process is spawned by the plugin.

### QQ and WeChat browser sessions

NetEase and KuGou advertise `qq` and `wechat` OAuth methods in addition to their native QR method. Their official web endpoints own the OAuth state and code exchange. The plugin's `account.auth.prepare-oauth` response therefore includes an optional `browserSession` policy with a canonical HTTPS root URL, allowed cookie names, required names, credential destination origins, and a credential format. This flow requires the matching updated desktop host.

Electron creates a fresh nonpersistent partition for the attempt and lets the official callback run. Cookie changes trigger a read filtered to the policy URL and names. Only the music-service session crosses the host-only `provider_auth_oauth_complete` command, in `browserSessionCookies`; QQ and WeChat cookies are excluded. Core checks the saved attempt and grants, normalizes the KuGou web session inside Core, and issues origin-bound credential handles. The guest receives `browserCredentials` entries containing only `origin` and `handle`, verifies the identity with the platform API, then commits the handles to its own Profile. Rejected sessions roll back newly imported credentials. The renderer cannot invoke this completion command.

For ordinary cookies, destinations must be the source host or one of its subdomains and must be granted. The KuGou format is pinned to its official web origin and three existing API origins. At most eight unique cookie names and four credential destinations are accepted; empty values, duplicate names, and header separators are rejected. The host clears browser storage and detaches cookie listeners after completion or cancellation. KuGou's declared HTTP callback is upgraded to the same HTTPS host and path before any authorization code is sent. Other HTTP navigation remains blocked.

This is an additive JSON-envelope change; it does not change the frozen WIT imports or expose a readable credential API. Spotify continues to use its existing host state and PKCE validation.

### Bounded response formats

Network requests opt into `responseFormat: "json"`. For a successful valid JSON response, the host returns a structured `bodyJson` instead of an escaped `body` string. HTTP errors and malformed JSON retain the legacy text body so error classification stays intact. The plugin falls back to the legacy string when an older host omits `bodyJson`.

Search uses typed platform envelopes and a visitor that retains only mapped track/artist/album/cover fields, skipping unrelated upstream metadata. It avoids repeated body decoding and array cloning. The original 10,000,000-fuel, 15-second, 64-MiB sandbox limits remain unchanged. Regression searches exercise first and second 20-item pages plus a 50-item page for both NetEase and KuGou. Parsing is O(B) in response bytes, with O(B + R) space for the wire response and retained result, rather than multiple complete metadata trees.

`catalog.artwork-data-uri` requests `responseFormat: "image"` through the same granted network import. The host accepts at most 256 KiB of PNG/JPEG/GIF/WebP data, checks MIME against the raster signature, and normalizes the observed NetEase `image/jpg` header to `image/jpeg`. SVG, HTML, oversized bodies, ungranted origins, and unsafe redirects are rejected. Binary range-based audio is unchanged.

YAQMC uses its existing `provider_cache_artwork` command with the song's source Provider/Profile, not the selected platform or the QQ-only artwork cache. Cache keys include that scope; denied external images are not retried as direct renderer URLs. NetEase requests a bounded 300×300 thumbnail. KuGou and Spotify image origins are explicitly declared in the manifest and require approval. Image encoding/decoding is O(image bytes); this is not a general browser network or file-access grant.

NetEase search `picUrl` may use HTTP even when song details later return HTTPS. The raw and compact search mappers normalize it once, upgrading only URLs that pass the existing official artwork-origin checks after changing the scheme. Lookalike hosts, userinfo, explicit ports and fragments are not normalized into accepted URLs. Other inputs remain unchanged for the existing artwork boundary to reject; HTTP/CSP permissions are not expanded. Normalization is O(URL bytes) time and space. Production checks require HTTPS in search results and a decoded player-bar image after aggregate row playback, rather than testing only details.

## Provider registration

The manifest declares three component-backed Providers:

- `org.yaqmc.providers.music.spotify`
- `org.yaqmc.providers.music.netease`
- `org.yaqmc.providers.music.kugou`

YAQMC registers all three in both modes. When `mode=aggregate`, it additionally registers `org.yaqmc.providers.music.aggregate`, whose members are the three external Providers plus the default YAQMC Provider.

The federation is a runtime view, not an independently installed platform. It is not pre-registered as an inactive descriptor at startup, and switching back to `isolated` removes its live and inactive entries. Mode checks cover the actual `provider_list`, not just live registry membership, including clean restarts in both modes.

Successful install, enable/disable, reload, uninstall, settings updates and global plugin-mode updates emit `plugin://changed` through the Core EventSink. The renderer refreshes its Provider list asynchronously without a page reload. Event payloads contain only the plugin ID and enabled flag, not settings, Client IDs or credentials. Failed mutations do not emit the event. A real duplex-protocol regression verifies delivery; UI regressions cover both directions of the mode switch. The notification adds O(1) work per successful mutation, excluding the existing Provider-list refresh.

### Native song references in aggregate mode

An aggregate song ID encodes its source Provider/Profile, source catalog ID and optional full `ProviderTrackReference`. The latter preserves raw track ID, numeric ID, album ID and media ID; QQ's catalog-prefixed ID is not interchangeable with its raw MID. These are public track metadata, never session data or credential handles.

Before source playback or cache insertion, the federation restores the source song ID, native reference and source album ID. Current-quality commands use the native track ID, not the aggregate ID or catalog-prefixed ID. Embedded references whose Provider/Profile differs from the route, or whose native track ID is empty, are rejected. Source routing still checks live federation membership; this is not an authorization bypass.

The optional field keeps old IDs decodable. Playback/current-quality operations for a legacy route without a native reference fetch source song details first; unresolved old references are not inserted into the source cache. New routes need no additional lookup. Encoding/decoding is O(L) time/space in reference length, member selection is O(P) for P members, and restoring cached songs is O(N × (L + P)) for N songs. The trade-off is longer opaque IDs in exchange for correct native metadata and avoiding an extra network request for newly returned songs.

The component receives `platform` from the selected manifest Provider. Account and catalog responses must return that exact Provider ID and the active Profile ID or the host rejects them as forged scope.

## Account flows

### Spotify

The component prepares an OAuth URL and callback matcher. YAQMC owns the authorization window, generates the host attempt ID/state, validates callback navigation, then sends the callback URL to the component. The component validates state and exchanges the code with its PKCE verifier.

The exact callback is `http://127.0.0.1:43821/oauth/spotify/callback`. YAQMC intercepts that navigation before Chromium reaches the network, so the plugin does not open a loopback listener or start a local service. Plain HTTP is accepted only for an explicit-port `127.0.0.1` callback; `localhost`, lookalike hosts, and all other HTTP URLs remain rejected.

Lyricify 4's public Custom Client guide is used only as evidence for a user-owned Spotify application and an exact `127.0.0.1` redirect. YAQMC does not reproduce Lyricify's unpublished authorization code or request its Client Secret. The plugin follows Spotify's public-client Authorization Code with PKCE flow and accepts only the user-supplied Client ID.

The host IPC call remains pending until authorization succeeds, fails, or is cancelled. On success it returns the completed account snapshot so the account dialog does not revert to a stale guest state.

External QR login URLs are not QQ mobile-app launch metadata. The account dialog keeps their platform-specific scan instructions and must not show “open QQ Music again” for NetEase or KuGou. Current-track quality controls send `provider_set_current_quality` using the playing song's Provider/Profile even if the selected catalog platform has changed.

### NetEase Cloud Music

The component requests `/api/login/qrcode/unikey`, renders `https://music.163.com/login?codekey=...`, and polls `/api/login/qrcode/client/login`.

Status mapping:

- `801`: waiting for scan
- `802`: waiting for confirmation
- `803`: authenticated
- `800`: expired

EAPI signs the `/api/...` path but sends to `https://interfacepc.music.163.com/eapi/...`. Its MD5 digest is lowercase and AES-ECB ciphertext is uppercase hexadecimal. The URL, digest, and ciphertext are checked against an independently generated upstream golden vector. Song URLs use `/api/song/enhance/player/url/v1` with `standard`, `exhigh`, `lossless`, `hires`, or `jymaster`; reported trial metadata marks a preview instead of implying a full resource.

### KuGou Music

The component requests `/v2/qrcode`, renders the KuGou H5 login URL, and polls `/v2/get_userinfo_qrcode` using the Web signature scheme.

Status mapping:

- `1`: waiting for scan
- `2`: waiting for confirmation
- `4`: authenticated
- `0`: expired

KuGou search results and the mobile song-detail response use different object shapes. The detail response is normalized through the same provider mapper before it crosses the YAQMC contract boundary; zero `timeLength` values fall back to `extra.128timelength`, and official image templates are upgraded to HTTPS with `{size}` resolved.

Playback consumes YAQMC's nested `song.provider.trackId` payload. The KuGou tracker requires Android signing and its own quality values (`128`, `320`, `flac`, `hires`), returns media URLs as an array, and declares media length in `fileSize`. The plugin upgrades only four explicitly granted KuGou CDN hosts to HTTPS; unlisted hosts are rejected rather than weakening the sandbox. Mobile `timeLength` is seconds, while `extra.128timelength` is milliseconds.

Lyrics first search `/v1/search` by the track hash, then request `/download` with the returned lyric ID and access key. A song hash is not a lyric ID.

The plugin also reads song details and supplies the title and millisecond duration to lyric search. Candidates must match the title (case-insensitive for ASCII, with a leading `artist - ` stripped from the detail title) and differ in duration by no more than 2,000 ms. Missing metadata or no matching candidate returns `null`; mismatched first candidates are not downloaded. This is a conservative heuristic, not proof of recording identity. It adds one detail request; selection scans c candidates in O(c × title bytes) time, with response-sized storage.

For authenticated KuGou requests, the component supplies a `credentialQuerySigning` binding using the `kugou-android` scheme. YAQMC resolves the origin-bound credential internally, inserts `token` and `userid`, recalculates the tracker key when present, and recalculates the Android signature. The binding requires account authority and an origin-bound Cookie header, accepts only GET requests to the granted gateway/tracker origins, and retains the cross-origin redirect fence. No credential-read import is exposed to the component. Signing is bounded to 64 query parameters, with O(n log n) time and O(n + query bytes) working space.

This requires the matching patched YAQMC portable build; older hosts do not perform credential query signing.

## Current catalog and quality boundaries

Authenticated account-library operations are implemented directly by the component:

- Spotify uses current `/v1/me/tracks`, `/v1/me/playlists`, and `/v1/playlists/{id}/items` endpoints.
- NetEase resolves the liked-song ID list through `/api/song/like/get`, then batches IDs into `/api/v3/song/detail`; playlists use `/api/user/playlist` and `/api/v6/playlist/detail`.
- KuGou uses the host-signed read-only `cloudlist` gateway for `/v7/get_all_list` and `/v4/get_list_all_file_v3`.
- KuGou favorite songs are intentionally unsupported because no stable public compatible endpoint was verified.
- Playlist-track requests validate Provider, Profile, and playlist reference, and pagination advances by consumed raw items to avoid cursor deadlock on null or filtered entries.

NetEase additionally implements home/discovery, public playlists, album/artist details, album/artist/playlist searches, and artist album pages. Artist top songs currently return up to 20 tracks. Public playlists return 20-track component pages with an optional `nextCursor`; the matching Core merges them with scope/identity checks and rejects repeated cursors or more than 250 pages. Spotify and KuGou still reject non-song search kinds and unsupported catalog operations. YAQMC routes search and entity pages independently of its home-feed load.

The matching host reads lyrics using the playing song's Provider/Profile and original track ID. Comments are loaded independently through `provider.comments`; older components may still use `LyricDocument.comments` with up to 15 popular comments. The lyric overlay only renders text and counts; it never posts or likes comments. A separate saved display switch controls the overlay. Older hosts ignore this additive field. NetEase account snapshots also read avatar/masked identity and verified VIP expiry; `music-vip` is distinct from QQ Green Diamond. Unknown or failed entitlement responses stay unknown and do not grant audio rights.

Endpoint definitions were checked against the upstream modules: [playlists](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/top_playlist.js), [charts](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/toplist.js), [lyrics](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/lyric.js), [popular comments](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/comment_hot.js), and [VIP information](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced/blob/master/module/vip_info.js). The actual Release Core/Component probe is recorded in TEST-RESULTS.md; source inspection alone is not endpoint verification.

Preferred-quality state is stored per platform/Profile in host KV. A single current-track override is retained per scope, discarded when resolving a different track, and does not create an unbounded per-song preference history. `catalog.status` returns the saved preference; YAQMC validates scope and bounds every projected capability to its host grants. Unsupported or older components retain the host fallback status.

`automatic` currently requests the standard upstream level. Spotify accepts only automatic/standard previews; KuGou rejects master. Selection reports requested and resolved levels separately: bitrate identifies lossy levels, while FLAC and verified sample-rate/bit-depth/level metadata identify lossless or higher tiers. It does not label a lower resource as the requested higher quality. Unknown resources and account rights remain unknown; successful login alone does not assert a membership tier.

KuGou tries at most four supported levels in descending order only when the upstream returns its explicit permission-denial status. A usable lower level continues playback with `account-rights` as its fallback reason; the denied requested level is marked denied. Network errors, malformed responses, and unrelated errors are returned without blind retries. This does not bypass rights checks or add alternate/unblock media sources. Preference lookup uses a bounded number of KV operations, and the quality fallback loop uses O(1) extra state and at most four upstream requests, excluding response buffers. Paid, lossless, Hi-Res, and master paths still require real-account/manual verification.

## Component format

Music plugin 0.2.2 declares `provider.comments` for NetEase and implements `comments.list`, requiring the matching YAQMC host. Top-level comments use `/api/v2/resource/comments` (`sortType=2` for popular and `3` for chronological); replies use `/api/resource/comment/floor/get`. The component returns bounded plain-text pages, author/avatar/date, like/reply counts and opaque cursors. YAQMC owns the drawer, portrait bottom sheet, lyrics overlay, saved switch, cancellation and shared caching. Lyrics no longer wait on a comment request. The older `LyricDocument.comments` host fallback remains compatible with earlier plugins. This API does not post comments or likes.

The guest is compiled for `wasm32-unknown-unknown` and encoded with `wit-component`. Compiling the guest directly for `wasm32-wasip2` is not supported: it adds WASI CLI imports that YAQMC intentionally does not grant.

`scripts/validate.ps1` rejects Core Wasm, unexpected WASI imports, native binaries, and legacy runtime files in the package.

### Comment cursor and reply limits

Cursors are versioned and bound to the exact song, profile, sort, parent comment and page size. A mismatched or pre-0.2.2 cursor is rejected before an upstream request. Reopen the comment panel after upgrading.

The floor endpoint only paginates oldest first. For `new` replies the plugin reads the complete thread (at most 500 upstream entries and ten 50-entry requests), then sorts by time descending with a stable comment-ID tie break. Keyset cursors prevent newer insertions or earlier deletions from shifting the next page. Reply pages advertise only `new`; unsupported reply sorts are rejected. Oversized or incomplete bounded reads return `comments-limit-exceeded` rather than a partial result labelled newest. The matching host displays this as a non-retryable reply-thread limit.

Reply-page processing is O(n log n) time and O(n) memory for n <= 500, excluding bounded network buffers. Each subsequent reply page revalidates the bounded thread; this intentionally avoids persisting public comment bodies in plugin storage. Top-level pages remain O(p) apart from ID deduplication, with p <= 50. All network, memory, fuel and host deadlines remain enforced.
