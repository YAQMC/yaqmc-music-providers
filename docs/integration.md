# YAQMC Integration

This repository contains the platform adapters and a local native backend.
The backend is intentionally separate from the YAQMC checkout. YAQMC loads
only the Wasm Provider Component and calls the backend through the loopback
network permission declared by the package.

## Start the backend

Run this before enabling a provider package:

```powershell
$env:SPOTIFY_CLIENT_ID = "your-client-id"
cargo run -p yaqmc-music-providers-backend -- init --path config.local.toml
cargo run -p yaqmc-music-providers-backend -- serve --config config.local.toml
```

Edit `config.local.toml` before the `serve` command if NetEase, KuGou, a
non-default data directory, or a different deployment mode is required. The
backend listens on `127.0.0.1:43821` by default.

The current YAQMC host must contain the narrowly-scoped loopback network
permission for `http://127.0.0.1:43821`. No native sidecar is started by the
plugin runtime, so starting the backend is a separate user action.

## Build and install

Build architecture-neutral packages with the supplied script:

```powershell
pwsh ./scripts/build.ps1 -Mode isolated
```

Install the resulting `dist/*.yaqmc-plugin` files through YAQMC's plugin
manager. Grant the permissions shown by the install review, especially the
provider capabilities, private storage, and loopback network origin.

In isolated mode, install any or all of:

- `org.yaqmc.providers.spotify`
- `org.yaqmc.providers.netease`
- `org.yaqmc.providers.kugou`

Each package registers one external platform. The built-in QQ Music provider
remains registered by YAQMC, so it can be used at the same time without
sharing cookies or account state with these packages.

To use the combined external catalog, build the aggregate variant:

```powershell
pwsh ./scripts/build.ps1 -Mode aggregate
```

Install exactly one package from that build. Every aggregate component calls
the same backend fan-out and installing all three would duplicate results.
The package's provider ID remains platform-specific for compatibility with
the current YAQMC registry; choose one package as the aggregate entrypoint and
do not install its two aggregate siblings. Login state is still keyed by
`(platform, profileId)` in the backend, so an aggregate search can use all
platforms that have been configured and authenticated.

## Login flows

Spotify exposes browser OAuth with PKCE. YAQMC should open the returned
authorization URL and complete the attempt using the fixed loopback callback.
NetEase and KuGou expose QR login when their configured API service supports
the corresponding endpoints. QR attempts can be polled, refreshed, or
cancelled; cancellation does not delete an existing profile session.

Use a distinct `profileId` for each independent account. Profile IDs are
opaque local labels and must not contain cookies, email addresses, or tokens.

## Request boundary

The Wasm component sends JSON requests to `POST http://127.0.0.1:43821/v1`.
The native backend performs upstream requests, stores platform credentials,
and converts playable upstream URLs into short-lived loopback media tokens.
The component never receives a Spotify secret, platform cookie, or host file
path. QQ Music is not proxied through this endpoint; it remains a YAQMC-host
provider.

## Troubleshooting

- `provider-not-configured`: set the platform endpoint and restart the backend.
- `authentication-required`: complete the platform login for the same
  `profileId` used by the request.
- `full-playback-unavailable`: the upstream API did not expose a bounded audio
  source. Spotify Web API search and library results do not guarantee full
  track playback; a preview may be available instead.
- `network origin is not granted`: reinstall or re-enable the package and
  approve the exact loopback permission, then verify that the backend uses
  port `43821`.
