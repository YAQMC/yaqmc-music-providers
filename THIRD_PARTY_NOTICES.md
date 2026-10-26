# Third-party notices

This file records public references used for protocol compatibility. The shipped Rust component is an independent implementation; no Node.js package or upstream runtime is included in the plugin archive.

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
| [MakcRe/KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) | `62730f900dc9fdc51396a663b3d33a6c4c704046` | MIT | KuGou artist/album/playlist search and details, recommendations, charts, POST body signing, official preview resources, and read-only song comment/reply request shapes; sort endpoints also checked against service responses; implementation independently rewritten |
| [chthollyphile/folia-major](https://github.com/chthollyphile/folia-major) | `1f2c32a5d91afd1446a9ca8e7d4305ca6f9f22ed` | MIT | KuGou search and mobile song-detail endpoint/response-shape reference; implementation independently rewritten |

The repository still contains historical sidecar experiments under `crates/backend`, `crates/platforms`, and `runtime`. They are not copied into the `.yaqmc-plugin` archive and are not required at runtime.
