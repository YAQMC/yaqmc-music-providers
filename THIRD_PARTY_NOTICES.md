# Third-party notices

This file records public references used for protocol compatibility and bundled playback assets. The shipped Rust component is an independent implementation. The archive also includes the plugin's browser player and hls.js; it contains no Node.js package or native browser/CDM runtime.

| Project | Revision / access date | License | Use |
| --- | --- | --- | --- |
| [Spotify Web API documentation](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow) | accessed 2026-10-05 | Documentation | Authorization Code with PKCE, token refresh, scopes, and Redirect URI requirements |
| [spotify/web-api-examples](https://github.com/spotify/web-api-examples) | `7c4872d343a6f29838c437cf163012947b4bffb9` | Apache-2.0 | Public-client PKCE request and token-exchange behavior; no source copied |
| [Spotify Web API reference](https://developer.spotify.com/documentation/web-api/reference) | accessed 2026-10-08 | Documentation | Current saved-tracks, current-user playlists, and playlist-items endpoints |
| [WXRIW/Lyricify-App](https://github.com/WXRIW/Lyricify-App) | `24e25d0f8df5a64d4047e9e533f5422efc593a5a` | No repository license declared | User-supplied Spotify application and exact loopback Redirect URI configuration only; authorization source code is not public |
| [NeteaseCloudMusicApiEnhanced/api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced) | `2aab9957dfd5231e5b192aecdb177f93ace4c92e` | MIT | QR key/check endpoints, status mapping, and response-shape compatibility |
| [NeteaseCloudMusicApiEnhanced/api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced) | `2aab9957dfd5231e5b192aecdb177f93ace4c92e`; `likelist`, `user_playlist`, `playlist_detail`, and `song_detail` rechecked 2026-10-08 | MIT | Account liked-list, user-playlist, playlist-detail, and batched song-detail endpoint shapes |
| [MakcRe/KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) | `da5ccfd9304c043085a2fd18e94ebc5c315044ab`; playback/lyric modules rechecked 2026-10-08 | MIT | QR endpoints/status mapping, Web and Android signatures, authenticated query signing, tracker quality/response shapes, and lyric candidate/download protocol; implementation independently rewritten |
| [MakcRe/KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) | `da5ccfd9304c043085a2fd18e94ebc5c315044ab`; `user_detail`, `cryptoRSAEncrypt`, `user_playlist`, and `playlist_track_all_new` rechecked 2026-10-08 | MIT | KuGou account detail, right-padded RSA signature, account playlist, and playlist-track endpoint shapes; implementation independently rewritten |
| [MakcRe/KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) | `62730f900dc9fdc51396a663b3d33a6c4c704046` | MIT | KuGou artist/album/playlist search and details, recommendations, charts, POST body signing, official preview resources, read-only song comments/replies, and KRC request/XOR/zlib format; sort endpoints also checked against service responses; implementation independently rewritten |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | `0.8.9` | MIT OR Zlib OR Apache-2.0 | Pure Rust, output-bounded zlib decompression for KRC lyrics inside the Wasm component |
| [guowenye/qishui-api](https://github.com/guowenye/qishui-api) | `e409c10fe7441a10d370da7da61d62bbc1e5126c`; routes rechecked 2026-11-20 | MIT | Qishui Music public discover, playlist-detail, H5 track/lyric, and PC search endpoints and response shapes; implementation independently written in Rust, no source copied. Its PlayAuth/spade decryption is deliberately not used |
| [margbug01/MySpotify](https://github.com/margbug01/MySpotify) | `f84e2e7537cbf6ea1dcba897d44b7e4d25cbf761`; account and catalog operations rechecked 2026-11-21 | AGPL-3.0 | Behavioral and protocol reference for Spotify web-player cookie token exchange, anonymous web-player token, profile/library Pathfinder queries, track/playlist catalog flow, Web Player media manifests, and Widevine license flow; independently implemented in Rust. No Go source, runtime, assets, or persisted account credentials are included |
| [chthollyphile/folia-major](https://github.com/chthollyphile/folia-major) | `1f2c32a5d91afd1446a9ca8e7d4305ca6f9f22ed` | MIT | KuGou search and mobile song-detail endpoint/response-shape reference; implementation independently rewritten |
| [YAQMC](https://github.com/YAQMC/YAQMC) | `crates/yaqmc-core/assets/protected_hls_player.html` | GPL-3.0-or-later | Browser player adapted into `plugins/music/assets/spotify-player.html`; corresponding source is that HTML file, distributed directly in the plugin. License included as `assets/player.LICENSE` |
| [video-dev/hls.js](https://github.com/video-dev/hls.js) | `1.7.4` | Apache-2.0 | Bundled as `assets/hls.min.js`, with upstream notices in `assets/hls.min.js.LICENSE` and license text in `assets/hls.LICENSE-APACHE` |

The repository still contains historical sidecar experiments under `crates/backend`, `crates/platforms`, and `runtime`. They are not copied into the `.yaqmc-plugin` archive and are not required at runtime.

## qishui-api license notice

```text
MIT License

Copyright (c) 2026 arrow

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
