# Configuration

The native backend reads a TOML file and keeps login state in the configured
data directory. Generate a baseline file with:

```powershell
cargo run -p yaqmc-music-providers-backend -- init --path config.local.toml
```

A minimal setup is:

```toml
dataDir = "data"
mode = "isolated"

[listen]
host = "127.0.0.1"
port = 43821

[spotify]
clientId = "paste-your-own-spotify-client-id"
redirectUri = "http://127.0.0.1:43821/oauth/spotify/callback"

[netease]
baseUrl = "https://music.163.com"

[kugou]
baseUrl = ""
```

## Spotify Client ID

The Spotify Client ID is always supplied by the user. It is accepted from
`spotify.clientId` in TOML or from `SPOTIFY_CLIENT_ID`; the environment
variable takes precedence when both are present. There is no client secret in
the OAuth flow and no credential is compiled into the component.

Register this exact redirect URI in the Spotify application settings:

```text
http://127.0.0.1:43821/oauth/spotify/callback
```

The backend refuses a different redirect URI because the callback is bound to
the local YAQMC integration boundary.

## Platform endpoints

NetEase and KuGou adapters target compatible HTTP JSON API services. The
backend does not bundle an API server. Set `NETEASE_API_BASE_URL` or
`KUGOU_API_BASE_URL` to override the corresponding TOML value. Base URLs must
be HTTPS, without credentials, query strings, or fragments. HTTP is accepted
only for a loopback fixture during development.

Spotify uses the official Web API endpoints by default. The optional
`spotify.apiBaseUrl` and `spotify.accountsBaseUrl` fields are intended for
controlled backend test fixtures and are subject to the same URL validation.
The packaged YAQMC component declares the official Spotify accounts origin for
OAuth navigation, so a custom accounts origin requires a matching package
manifest and is not a drop-in production override.

## Modes

The component mode is selected when the package is built:

```powershell
pwsh ./scripts/build.ps1 -Mode isolated
pwsh ./scripts/build.ps1 -Mode aggregate
```

`isolated` sends each provider request to one platform. `aggregate` fans out
catalog and library requests across all configured external platforms and
preserves `(platform, profileId, trackId)` in every result. The backend's
`mode` field describes the intended deployment and is also used by direct
protocol clients; packaged components carry their selected mode in the Wasm
build.

Install all three packages from an isolated build when each platform should
appear separately. For an aggregate build, install only one of the generated
provider packages, otherwise the same fan-out results are registered more
than once. QQ Music is still supplied by YAQMC's built-in provider and is
used alongside the external provider package rather than through the native
backend.

## Stored credentials

The backend stores OAuth tokens and platform cookies in
`<dataDir>/profiles.json`. Treat that file as a secret, keep the data
directory private, and do not add it to Git. `httpAuthToken` is available for
clients that can send an Authorization header; the packaged component uses
the loopback boundary and currently leaves this option unset.
