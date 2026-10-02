# YAQMC Music Providers

Independent provider adapters for YAQMC. The repository keeps platform protocol
code and login state outside the YAQMC application repository.

## Scope

The first release targets Windows and Linux and contains three separately
installable provider packages:

- Spotify
- NetEase Cloud Music
- KuGou Music

Each provider can run in **isolated mode**. The backend also exposes an
**aggregate mode** that fans out bounded catalog and library requests and keeps
the source `(platform, profileId, trackId)` on every result. QQ Music remains
the built-in YAQMC provider and runs alongside these external providers; it is
not proxied through this backend.

Spotify uses a user-supplied Client ID. No Client ID, client secret, access
token, refresh token, or test account is committed to this repository.

## Current integration boundary

The backend is a local loopback service with optional bearer authentication.
YAQMC's current Provider
Component sandbox only permits declared origins and cannot start a native
sidecar, so the repository ships the backend and provider adapters
independently. `docs/integration.md` documents the smallest host boundary
needed to connect a packaged provider to the loopback service. The plugin
repository does not modify the YAQMC checkout.

Full Spotify audio playback requires a Spotify playback session and a Premium
account. Web API search and library access do not provide a general-purpose
audio URL; the backend reports that distinction explicitly instead of treating
a preview URL as a full track.

## Quick start

```powershell
$env:SPOTIFY_CLIENT_ID = "your-client-id"
cargo run -p yaqmc-music-providers-backend -- init --path config.local.toml
cargo run -p yaqmc-music-providers-backend -- serve --config config.local.toml
```

Run the deterministic protocol and adapter tests with:

```powershell
cargo test --workspace --locked
```

See `docs/configuration.md`, `docs/integration.md`, and `THIRD_PARTY_NOTICES.md`
for configuration, security boundaries, and borrowed reference revisions.

Build architecture-neutral Provider Component packages with:

```powershell
pwsh ./scripts/build.ps1 -Mode isolated
```

Use `-Mode aggregate` to build the fan-out variant. Install all three
isolated packages for separate platform entries. Install only one aggregate
package, because every aggregate package queries the same configured external
platforms. QQ Music remains YAQMC's built-in provider in both modes.
