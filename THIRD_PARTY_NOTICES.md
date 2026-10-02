# Third-party notices

This file records source references used for protocol compatibility. The
implementation in this repository is an independent adapter unless a notice
below explicitly says otherwise. Reference revisions are pinned so a future
protocol change can be reviewed rather than silently absorbed.

| Project | Revision | License | Use |
| --- | --- | --- | --- |
| [NeteaseCloudMusicApiEnhanced/api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced) | `4a45a14aa7e035e22b824600409ab1a07b748a0e` | MIT | Endpoint names and response-shape compatibility |
| [MakcRe/KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) | `da5ccfd9304c043085a2fd18e94ebc5c315044ab` | MIT | Endpoint names and login/result compatibility |
| [librespot-org/librespot](https://github.com/librespot-org/librespot) | `939dc5ee9d833e1980f9495241219d9d4868a061` | MIT | Planned Spotify Connect/librespot playback adapter |
| [aome510/spotify-player](https://github.com/aome510/spotify-player) | repository state reviewed 2026-10-02 | MIT | PKCE and Web API/session separation design reference |

No source file is copied from these projects in the initial implementation.
When code is copied in a later change, its copyright and license text must be
added here before merging that change.
