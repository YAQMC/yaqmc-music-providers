# Plugin configuration

The current plugin is self-contained. It does not read a TOML file and does not require a local backend.

## Settings

| Field | Values | Purpose |
| --- | --- | --- |
| `mode` | `isolated`, `aggregate` | Toggles the additional federation Provider. Individual Providers remain available in both modes. |
| `profileId` | non-empty string | Default credential namespace used by the component. |
| `spotifyClientId` | Spotify application Client ID | Enables Spotify OAuth 2.0 PKCE. |

Settings are stored and supplied by YAQMC. A Client Secret is neither required nor accepted.

## Spotify application

Create a Web API application in Spotify Developer Dashboard and add this exact Redirect URI:

```text
http://127.0.0.1:43821/oauth/spotify/callback
```

The host intercepts the callback navigation in the authorization window; no companion HTTP service listens on this port.

The authorization request uses:

- Authorization Code flow with PKCE (`S256`)
- a random verifier and state per attempt
- `user-read-private`
- `user-read-email`
- `user-library-read`
- `playlist-read-private`
- `playlist-read-collaborative`

## Credentials

YAQMC stores OAuth tokens and platform cookies in its credential store. The component KV contains only opaque credential handles, expiry timestamps, device identifiers, and short-lived login attempt state.

Credential namespaces include both Provider ID and Profile ID. Signing out one platform does not delete another platform's credentials.

## Network permissions

The manifest grants only the platform origins used by the component. There are no loopback API permissions. Changes to an upstream origin require a manifest permission change and renewed user approval.
