import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { createRequire } from 'node:module';

const [coreInput, pluginInput, keepProfile] = process.argv.slice(2);
const loginPlatform = ['--login-netease', '--login-kugou'].includes(keepProfile) ? keepProfile.slice(8) : undefined;
const browserLogin = /^--browser-(netease|kugou)-(qq|wechat)$/.exec(keepProfile ?? '');
const catalogProbe = keepProfile === '--catalog-netease';
const prepareUi = keepProfile === '--prepare-ui-netease';
if (!coreInput || !pluginInput || (keepProfile && keepProfile !== '--keep-profile' && !loginPlatform && !browserLogin && !catalogProbe && !prepareUi)) {
  throw new Error('Usage: node scripts/test-host-smoke.mjs <core.exe> <plugin.yaqmc-plugin> [--keep-profile | --login-netease | --login-kugou]');
}
const coreSourcePath = resolve(coreInput);
const pluginPath = resolve(pluginInput);
const uiProfile = keepProfile === '--keep-profile' || prepareUi;
const permissions = JSON.parse(
  readFileSync(new URL('../plugins/music/manifest.json', import.meta.url), 'utf8'),
).permissions;
const temporaryRoot = resolve(tmpdir());
const root = mkdtempSync(join(temporaryRoot, 'yaqmc-provider-smoke-'));
// Run a private copy so an interactive login does not lock the build output.
const corePath = join(root, basename(coreSourcePath));
copyFileSync(coreSourcePath, corePath);
let electronApp;
if (browserLogin) {
  if (!process.versions.electron) throw new Error('Browser login tests must be launched with Electron');
  const {app} = createRequire(import.meta.url)('electron');
  electronApp = app;
  app.setPath('userData', join(root, 'electron'));
  // Keep the driver alive after the authorization window closes so it can
  // verify persistence, read playlists, sign out, and stop its private Core.
  app.on('window-all-closed', () => {});
  app.commandLine.appendSwitch('proxy-server', 'http://127.0.0.1:10808');
}
const dataRoot = prepareUi ? join(root,'core-data') : uiProfile ? join(root, 'AppData', 'Roaming', 'org.yaqmc.desktop') : join(root, 'data');
const cacheRoot = prepareUi ? join(root,'cache') : uiProfile ? join(root, 'AppData', 'Local', 'org.yaqmc.desktop') : join(root, 'cache');
mkdirSync(dataRoot, {recursive:true});
mkdirSync(cacheRoot, {recursive:true});
for (const name of ['data', 'cache', 'logs', 'config', 'credentials', 'plugins', 'downloads']) {
  mkdirSync(join(root, name), { recursive: true });
}

function startCore() {
  return spawn(corePath, [], {
  cwd: root,
  windowsHide: true,
  env: {
    ...process.env,
    YAQMC_DATA_DIR: dataRoot,
    YAQMC_CACHE_DIR: cacheRoot,
    YAQMC_LOG_DIR: join(root, 'logs'),
    YAQMC_CONFIG_DIR: uiProfile && !prepareUi ? dataRoot : join(root, 'config'),
    YAQMC_CREDENTIAL_DIR: prepareUi ? join(dataRoot,'credentials') : join(root, 'credentials'),
    YAQMC_PLUGIN_FALLBACK_DIR: join(root, 'plugins'),
    YAQMC_LOG_FALLBACK_DIR: join(root, 'logs'),
    YAQMC_DOWNLOAD_DIR: join(root, 'downloads'),
    HTTP_PROXY: 'http://127.0.0.1:10808',
    HTTPS_PROXY: 'http://127.0.0.1:10808',
    NO_PROXY: '127.0.0.1,localhost',
  },
  stdio: ['pipe', 'pipe', 'pipe'],
  });
}

let child = startCore();

let stderr = '';
let buffer = Buffer.alloc(0);
const queued = [];
const waiting = [];
function watchCore() {
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => (stderr += chunk));
  child.stdout.on('data', (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (buffer.length >= 4) {
    const length = buffer.readUInt32LE(0);
    if (buffer.length < length + 4) break;
    const message = JSON.parse(buffer.subarray(4, length + 4).toString('utf8'));
    buffer = buffer.subarray(length + 4);
    const waiter = waiting.shift();
    if (waiter) waiter(message);
    else queued.push(message);
  }
  });
}
watchCore();

function send(message) {
  const body = Buffer.from(JSON.stringify(message));
  const header = Buffer.alloc(4);
  header.writeUInt32LE(body.length);
  child.stdin.write(Buffer.concat([header, body]));
}

function receive(timeoutMs = 120000) {
  if (queued.length) return Promise.resolve(queued.shift());
  return new Promise((resolve, reject) => {
    const waiter = (message) => {
      clearTimeout(timer);
      resolve(message);
    };
    const timer = setTimeout(() => {
      const index = waiting.indexOf(waiter);
      if (index >= 0) waiting.splice(index, 1);
      reject(new Error(`receive timeout; stderr=${stderr}`));
    }, timeoutMs);
    waiting.push(waiter);
  });
}

let nextId = 1;
async function request(method, params, timeoutMs = 120000, origin = 'main') {
  const id = nextId++;
  send({ kind: 'request', id, method, params, origin });
  for (;;) {
    const message = await receive(timeoutMs);
    if (message.kind === 'event') continue;
    if (message.kind !== 'response' || message.id !== id) {
      throw new Error(`unexpected ${method} response: ${JSON.stringify(message)}`);
    }
    if (message.result !== undefined) return message.result;
    throw new Error(`${method} failed: ${JSON.stringify(message.error)}`);
  }
}

async function assertProviderMode(mode) {
  const settings = await request('plugin_settings_get', {id: 'org.yaqmc.providers.music'});
  const providers = await request('provider_list');
  const ids = providers.map(item => item.id ?? item.providerId);
  const aggregate = ids.includes('org.yaqmc.providers.music.aggregate');
  const expected = [
    'qqmusic',
    ...['spotify', 'netease', 'kugou'].map(platform => `org.yaqmc.providers.music.${platform}`),
  ];
  if (mode === 'aggregate') expected.push('org.yaqmc.providers.music.aggregate');
  if (
    settings.mode !== mode ||
    aggregate !== (mode === 'aggregate') ||
    ids.length !== expected.length ||
    expected.some(id => !ids.includes(id)) ||
    providers.some(item => !item.available)
  ) {
    throw new Error(`Provider mode mismatch: expected ${mode}, settings=${settings.mode}, ids=${JSON.stringify(ids)}`);
  }
  return ids;
}

let playbackAvailable = true;

async function testSearchPages(providerId, query) {
  const pages = [];
  for (const { page, limit } of [{ page: 1, limit: 20 }, { page: 2, limit: 20 }, { page: 1, limit: 50 }]) {
    const result = await request('provider_search', {
      providerId, profileId: 'default', kind: 'song', query, page, limit,
    }).catch(error => {
      throw new Error(`${providerId} search page=${page}, limit=${limit}: ${error.message}`);
    });
    if (
      result.kind !== 'song' || result.query !== query || result.page !== page ||
      !Array.isArray(result.items) || result.items.length > limit ||
      (page === 1 && result.items.length === 0) ||
      result.items.some(item => item.provider?.providerId !== providerId || item.provider?.profileId !== 'default' ||
        (item.artwork?.src && !item.artwork.src.startsWith('https://')))
    ) {
      throw new Error(`${providerId} returned an invalid search page: page=${page}, limit=${limit}`);
    }
    pages.push({ page, limit, count: result.items.length, hasMore: result.hasMore });
  }
  return pages;
}

async function testPlatform(providerId, query, checkQuality = false, selectSong = () => true) {
  const search = await request('provider_search', {
    providerId,
    profileId: 'default',
    query,
    kind: 'song',
    page: 1,
    limit: 20,
  });
  const first = search.items?.find(selectSong);
  if (!first?.id) throw new Error(`${providerId} did not return the expected sample for ${query}`);
  if (!first.artwork?.src?.startsWith('https://')) {
    throw new Error(`${providerId} search sample does not have a cacheable HTTPS cover`);
  }
  const song = await request('provider_song', {
    providerId,
    profileId: 'default',
    id: first.id,
  });
  const image = await request('provider_cache_artwork', {
    providerId, profileId: 'default', url: song.artwork.src,
  });
  if (!/^data:image\/(png|jpeg|webp|gif);base64,[a-z0-9+/]+=*$/i.test(image)) {
    throw new Error(`${providerId} did not return a supported artwork data URI`);
  }
  const artworkBytes = Buffer.from(image.split(',')[1], 'base64').length;
  let lyrics;
  let lyricError;
  try {
    lyrics = await request('provider_lyrics', {
      providerId,
      profileId: 'default',
      id: song.id,
    });
  } catch (error) {
    lyricError = error.message;
  }
  if (!playbackAvailable) {
    return {
      id: song.id,
      title: song.title,
      durationMs: song.durationMs,
      lyricLines: lyrics?.lines?.length ?? 0,
      lyricSync: lyrics?.syncMode,
      lyricError,
      playbackNotTested: 'audio-output-unavailable',
    };
  }
  let playing;
  let advanced;
  let playbackError;
  let qualitySwitch;
  let qualitySwitchError;
  try {
    playing = await request(
    'player_play_tracks',
    { request: { tracks: [song], startAtId: song.id, shuffle: false } },
    180000,
  );
  if (playing.playbackState !== 'playing' || !playing.sourceSelection) {
    throw new Error(`${providerId} did not enter playing state: ${JSON.stringify(playing.playbackError)}`);
  }
  await new Promise((resolve) => setTimeout(resolve, 1500));
  advanced = await request('player_snapshot');
  if (advanced.playbackState !== 'playing' || advanced.positionMs <= 0) {
    throw new Error(`${providerId} playback clock did not advance`);
  }
  if (checkQuality) {
    try {
      const switched = await request('provider_set_current_quality', {
        providerId, profileId: 'default', quality: 'high',
      });
      if (switched.sourceSelection?.requestedQuality !== 'high' || switched.playbackState !== 'playing') {
        throw new Error('current-quality switch did not update the playback selection');
      }
      await new Promise(resolve => setTimeout(resolve, 1500));
      const refreshed = await request('player_snapshot');
      if (refreshed.playbackState !== 'playing' || refreshed.positionMs <= 0) {
        throw new Error('playback did not continue after switching quality');
      }
      qualitySwitch = {
        requestedQuality: refreshed.sourceSelection?.requestedQuality,
        resolvedQuality: refreshed.sourceSelection?.resolvedQuality,
        fallbackReason: refreshed.sourceSelection?.fallbackReason,
        positionMs: refreshed.positionMs,
      };
    } catch (error) {
      qualitySwitchError = error.message;
    }
  }
  } catch (error) {
    playbackError = error.message;
  }
  await request('player_pause');
  await request('player_hydrate_queue', { tracks: [] });
  return {
    id: song.id,
    title: song.title,
    searchArtworkScheme: 'https',
    artworkBytes,
    candidates: search.items.map(item => ({id:item.id,title:item.title})),
    durationMs: song.durationMs,
    lyricLines: lyrics?.lines?.length ?? 0,
    lyricSync: lyrics?.syncMode,
    lyricError,
    resolvedQuality: playing?.sourceSelection?.resolvedQuality,
    preview: playing?.sourceSelection?.preview,
    advancedPositionMs: advanced?.positionMs,
    playbackError,
    qualitySwitch,
    qualitySwitchError,
  };
}

async function testQr(providerId) {
  const scope = {providerId,profileId:'default'};
  const started = await request('provider_auth_start', scope);
  if (!started.qrImageDataUri?.startsWith('data:image/png;base64,')) throw new Error('missing QR image');
  const states = [];
  for (let heartbeat = 0; heartbeat < 3; heartbeat++) {
    await new Promise(resolve => setTimeout(resolve, 1500));
    const snapshot = await request('provider_auth_heartbeat', {...scope,attemptId:started.attemptId,ownerLeaseId:started.ownerLeaseId});
    if (snapshot.state !== 'waiting-for-scan' || snapshot.qrImageDataUri !== started.qrImageDataUri || snapshot.attemptId !== started.attemptId || snapshot.ownerLeaseId !== started.ownerLeaseId) throw new Error('QR state changed before scan');
    states.push(snapshot.state);
  }
  await request('provider_auth_cancel', {...scope,attemptId:started.attemptId});
  return {imageBytes:started.qrImageDataUri.length,heartbeats:states};
}

function sourceRoute(song) {
  const id = song.provider?.trackId;
  if (!id?.startsWith('yaqmc-federated:')) return undefined;
  return JSON.parse(Buffer.from(id.slice('yaqmc-federated:'.length), 'base64url').toString('utf8'));
}

async function testSpotifyPreparation() {
  const scope = {providerId: 'org.yaqmc.providers.music.spotify', profileId: 'default'};
  const methods = await request('provider_account_login_methods', scope);
  if (!methods.some(item => item.id === 'browser-oauth' && item.flow === 'oauth')) {
    throw new Error('Spotify PKCE login method is missing');
  }
  let rendererPrepareDenied = false;
  try {
    await request('provider_auth_oauth_prepare', {...scope, methodId: 'browser-oauth'});
  } catch (error) {
    if (!error.message.includes('host.denied')) throw error;
    rendererPrepareDenied = true;
  }
  if (!rendererPrepareDenied) throw new Error('Renderer origin accessed host-only OAuth preparation');
  let missingClientRejected = false;
  try {
    await request('provider_auth_oauth_prepare', {...scope, methodId: 'browser-oauth'}, 120000, 'host');
  } catch (error) {
    if (!error.message.includes('provider-not-configured')) throw error;
    missingClientRejected = true;
  }
  if (!missingClientRejected) throw new Error('Spotify accepted OAuth without a user Client ID');
  const fixtureClientId = '00000000000000000000000000000000';
  try {
    await request('plugin_settings_set', {request: {id: 'org.yaqmc.providers.music', values: {spotifyClientId: fixtureClientId}}});
    const prepared = await request('provider_auth_oauth_prepare', {...scope, methodId: 'browser-oauth'}, 120000, 'host');
    const url = new URL(prepared.url);
    const callback = 'http://127.0.0.1:43821/oauth/spotify/callback';
    if (
      url.origin !== 'https://accounts.spotify.com' ||
      url.pathname !== '/authorize' ||
      url.searchParams.get('client_id') !== fixtureClientId ||
      url.searchParams.get('response_type') !== 'code' ||
      url.searchParams.get('redirect_uri') !== callback ||
      url.searchParams.get('code_challenge_method') !== 'S256' ||
      !/^[A-Za-z0-9_-]{43}$/.test(url.searchParams.get('code_challenge') ?? '') ||
      !url.searchParams.get('state') ||
      JSON.stringify(prepared.navigationAllowlist) !== JSON.stringify(['https://accounts.spotify.com/**']) ||
      prepared.callbackMatcher?.urlPrefix !== callback
    ) {
      throw new Error('Spotify preparation did not preserve the configured client, PKCE, and exact callback');
    }
    const newer = await request('provider_auth_oauth_prepare', {...scope, methodId: 'browser-oauth'}, 120000, 'host');
    if (newer.attemptId === prepared.attemptId || new URL(newer.url).searchParams.get('state') === url.searchParams.get('state')) {
      throw new Error('Spotify attempts reused the same ID or state');
    }
    let staleCancelRejected = false;
    try {
      await request('provider_auth_oauth_cancel', {...scope, attemptId: prepared.attemptId}, 120000, 'host');
    } catch (error) {
      if (!error.message.includes('invalid-request')) throw error;
      staleCancelRejected = true;
    }
    if (!staleCancelRejected) throw new Error('Closing the previous OAuth window cancelled the current attempt');
    const cancelled = await request('provider_auth_oauth_cancel', {...scope, attemptId: newer.attemptId}, 120000, 'host');
    if (cancelled.state !== 'cancelled' || cancelled.attemptId !== newer.attemptId) {
      throw new Error('The current OAuth attempt did not survive stale cancellation');
    }
    const repeated = await request('provider_auth_oauth_cancel', {...scope, attemptId: newer.attemptId}, 120000, 'host');
    if (repeated.state !== 'guest') throw new Error('Repeated OAuth cancellation did not return the current account state');
    return {method: 'browser-oauth', rendererPrepareDenied, missingClientRejected, fixtureClientPreparation: true, staleCancelRejected, pkce: 'S256', callback, realAuthorization: 'not-tested'};
  } finally {
    await request('plugin_settings_set', {request: {id: 'org.yaqmc.providers.music', values: {spotifyClientId: ''}}});
  }
}

async function testBrowserPreparation() {
  const result = {};
  for (const platform of ['netease', 'kugou']) {
    const scope = {providerId: `org.yaqmc.providers.music.${platform}`, profileId: 'default'};
    const methods = await request('provider_account_login_methods', scope);
    for (const method of ['qq', 'wechat']) {
      if (!methods.some(value => value.id === method && value.flow === 'oauth')) throw new Error(`${platform} is missing ${method} browser login`);
      const prepared = await request('provider_auth_oauth_prepare', {...scope, methodId: method}, 120000, 'host');
      const source = platform === 'netease' ? 'https://music.163.com/' : 'https://openplat-user.kugou.com/';
      if (prepared.browserSession?.url !== source || !prepared.url.startsWith(source) || !prepared.callbackMatcher?.urlPrefix.startsWith(source)) throw new Error('Browser preparation has an unexpected service origin');
      let rendererDenied = false;
      try {
        await request('provider_auth_oauth_complete', {...scope, attemptId: prepared.attemptId, callbackUrl: source, browserSessionCookies: []});
      } catch (error) { if (!error.message.includes('host.denied')) throw error; rendererDenied = true; }
      if (!rendererDenied) throw new Error('Renderer accessed browser session completion');
      const cancelled = await request('provider_auth_oauth_cancel', {...scope, attemptId: prepared.attemptId}, 120000, 'host');
      if (cancelled.state !== 'cancelled') throw new Error('Browser login cancellation failed');
    }
    result[platform] = {methods: ['qq', 'wechat'], prepareAndCancel: true, rendererCompletionDenied: true};
  }
  return result;
}

let ready = false;
async function attachCore() {
  const hello = await receive(30000);
  if (hello.kind !== 'hello') throw new Error(`unexpected hello: ${JSON.stringify(hello)}`);
  send({
    kind: 'attach',
    protocol: 1,
    host: { app: 'yaqmc-playback-probe', version: '0.1.0' },
    platform: { mainWindowHandle: null, platformKind: 'windows' },
  });
  const attached = await receive(30000);
  if (attached.kind !== 'ready') throw new Error(`unexpected ready: ${JSON.stringify(attached)}`);
  ready = true;
}

async function stopCore() {
  if (ready && child.exitCode === null) {
    send({ kind: 'shutdown', reason: 'quit' });
    try { await receive(5000); } catch {}
  }
  child.stdin.end();
  if (child.exitCode === null) {
    await Promise.race([once(child, 'exit'), new Promise(resolve => setTimeout(resolve, 5000))]);
  }
  if (child.exitCode === null) {
    child.kill();
    await once(child, 'exit');
  }
  ready = false;
}

async function restartCore() {
  await stopCore();
  buffer = Buffer.alloc(0);
  queued.length = 0;
  waiting.length = 0;
  child = startCore();
  watchCore();
  await attachCore();
}

async function testInteractiveLogin(platform) {
  const scope = {providerId: `org.yaqmc.providers.music.${platform}`, profileId: 'default'};
  const initial = await request('provider_account_snapshot', scope);
  if (initial.state !== 'guest') throw new Error('Interactive login requires an isolated guest profile');
  const started = await request('provider_auth_start', scope);
  if (!started.qrImageDataUri?.startsWith('data:image/png;base64,')) throw new Error('missing QR image');
  const qrPath = join(root, `${platform}-login.png`);
  writeFileSync(qrPath, Buffer.from(started.qrImageDataUri.split(',')[1], 'base64'));
  process.stdout.write(`${JSON.stringify({platform, state: started.state, qrPath})}\n`);
  const deadline = Math.min(Date.now() + 300000, started.expiresAtMs || Infinity);
  let phase = started.state;
  let authenticated = false;
  try {
    while (Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 2000));
      let snapshot;
      try {
        snapshot = await request('provider_auth_heartbeat', {...scope, attemptId: started.attemptId, ownerLeaseId: started.ownerLeaseId});
      } catch (error) {
        // Never print a raw upstream error or a callback carrying credentials.
        const codes = [...String(error.message).matchAll(/"code"\s*:\s*"([a-zA-Z0-9_.-]+)"/g)].map(match => match[1]);
        process.stdout.write(`${JSON.stringify({platform, state: 'poll-error', codes})}\n`);
        continue;
      }
      if (snapshot.state !== phase) {
        phase = snapshot.state;
        process.stdout.write(`${JSON.stringify({platform, state: phase})}\n`);
      }
      if (phase === 'authenticated') {
        authenticated = true;
        break;
      }
      if (['expired', 'cancelled', 'rejected'].includes(phase)) break;
    }
    if (!authenticated) {
      process.exitCode = 2;
      process.stdout.write(`${JSON.stringify({platform, realAuthorization: 'not-completed', state: phase})}\n`);
      return;
    }
    await restartCore();
    const restored = await request('provider_account_snapshot', scope);
    if (restored.state !== 'authenticated') throw new Error('Session did not survive Core restart');
    const refreshed = await request('provider_account_refresh', scope);
    if (refreshed.state !== 'authenticated') throw new Error('Upstream rejected the restored session');
    const page = await request('provider_account_playlists', {...scope, cursor: null, limit: 20});
    if (!Array.isArray(page.items)) throw new Error('Account playlists did not return a page');
    for (const other of ['spotify', 'netease', 'kugou'].filter(value => value !== platform)) {
      const untouched = await request('provider_account_snapshot', {providerId: `org.yaqmc.providers.music.${other}`, profileId: 'default'});
      if (untouched.state !== 'guest') throw new Error('Login affected another platform account');
    }
    process.stdout.write(`${JSON.stringify({platform, realAuthorization: 'passed', restored: true, refreshed: true, playlistCount: page.items.length, otherPlatformsRemainGuest: true})}\n`);
  } finally {
    if (authenticated) {
      const signedOut = await request('provider_sign_out', scope);
      if (signedOut.state !== 'guest') throw new Error('Test session sign-out failed');
      process.stdout.write(`${JSON.stringify({platform, signedOut: true})}\n`);
    } else {
      await request('provider_auth_cancel', {...scope, attemptId: started.attemptId}).catch(() => undefined);
    }
  }
}

async function testInteractiveBrowserLogin(platform, method) {
  const {BrowserWindow, nativeImage, session} = createRequire(import.meta.url)('electron');
  const {openProviderOAuthWindow} = createRequire(import.meta.url)(resolve(process.env.YAQMC_OAUTH_TEST_MODULE));
  await electronApp.whenReady();
  const scope = {providerId: `org.yaqmc.providers.music.${platform}`, profileId: 'default'};
  let authenticated = false;
  const authorization = await openProviderOAuthWindow({providerId: scope.providerId, methodId: method}, {
    createWindow: options => {
      const window = new BrowserWindow(options);
      for (const event of ['will-navigate', 'will-redirect']) {
        window.webContents.on(event, (_event, url) => {
          try { const parsed = new URL(url); process.stdout.write(`${JSON.stringify({platform, method, event, protocol: parsed.protocol, host: parsed.hostname, path: parsed.pathname})}\n`); } catch {}
        });
      }
      window.webContents.on('did-fail-load', (_event, code, description, _url, mainFrame) => {
        process.stdout.write(`${JSON.stringify({platform, method, event: 'load-failure', code, description, mainFrame})}\n`);
      });
      return window;
    },
    fromPartition: (partition, options) => {
      const target = session.fromPartition(partition, options);
      target.setPermissionCheckHandler(() => false);
      target.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
      return target;
    },
    isPackaged: true,
    provider_auth_oauth_prepare: input => request('provider_auth_oauth_prepare', {...scope, ...input}, 120000, 'host'),
    provider_auth_oauth_complete: input => request('provider_auth_oauth_complete', {...scope, ...input}, 120000, 'host'),
    provider_auth_oauth_cancel: input => request('provider_auth_oauth_cancel', {...scope, ...input}, 120000, 'host'),
  });
  process.stdout.write(`${JSON.stringify({platform, method, state: 'browser-opened'})}\n`);
  try {
    const snapshot = await authorization.completion;
    if (snapshot.state !== 'authenticated') { process.exitCode = 2; process.stdout.write(`${JSON.stringify({platform, method, realAuthorization: 'not-completed', state: snapshot.state})}\n`); return; }
    authenticated = true;
    process.stdout.write(`${JSON.stringify({platform, method, state: 'authenticated', avatar: !!snapshot.profile?.avatarUrl, maskedIdentity: snapshot.profile?.maskedIdentity !== 'authenticated', tier: snapshot.entitlement?.tier, membership: snapshot.entitlement?.membership, hasExpiry: !!snapshot.entitlement?.expiresAtMs})}\n`);
    if (snapshot.profile?.avatarUrl) {
      const avatarOrigin = new URL(snapshot.profile.avatarUrl).origin;
      const value = await request('provider_cache_artwork', {...scope, url: snapshot.profile.avatarUrl});
      const decoded = nativeImage.createFromDataURL(value);
      if (decoded.isEmpty()) throw new Error('Account avatar did not decode');
      process.stdout.write(`${JSON.stringify({platform, method, state: 'avatar-decoded', origin: avatarOrigin, ...decoded.getSize()})}\n`);
    } else throw new Error('Account avatar URL is missing');
    const page = await request('provider_account_playlists', {...scope, cursor: null, limit: 20});
    if (!Array.isArray(page.items)) throw new Error('Account playlists did not return a page');
    const playlistTrackCounts = [];
    for (const playlist of [...page.items].sort((a, b) => b.trackCount - a.trackCount).slice(0, 3)) {
      const detail = await request('provider_account_playlist_tracks', {...scope, playlist, cursor: null, limit: 20});
      if (!Array.isArray(detail.tracks?.items)) throw new Error('Playlist tracks did not return a page');
      const playlistTrackCount = detail.tracks.items.length;
      playlistTrackCounts.push(playlistTrackCount);
      if (playlist.trackCount > 0 && playlistTrackCount === 0) throw new Error('A populated playlist returned no tracks');
    }
    process.stdout.write(`${JSON.stringify({platform, method, state: 'playlists-read', playlistCount: page.items.length, playlistTrackCounts})}\n`);
    process.stdout.write(`${JSON.stringify({platform, method, state: 'restarting-core'})}\n`);
    await restartCore();
    // Account restoration runs asynchronously after attach. Wait for it to
    // settle instead of treating the initial guest snapshot as lost storage.
    const restoreDeadline = Date.now() + 30000;
    let restored = await request('provider_account_snapshot', scope);
    let restoreSnapshots = 1;
    while (restored.state === 'guest' && Date.now() < restoreDeadline) {
      await new Promise(resolve => setTimeout(resolve, 1000));
      restored = await request('provider_account_snapshot', scope);
      restoreSnapshots++;
    }
    if (restored.state !== 'authenticated') throw new Error(`Session did not survive Core restart (state: ${restored.state})`);
    process.stdout.write(`${JSON.stringify({platform, method, realAuthorization: 'passed', restored: true, restoreSnapshots, playlistCount: page.items.length, playlistTrackCounts})}\n`);
  } catch (error) {
    process.exitCode = 2;
    const codes = [...String(error.message).matchAll(/"code"\s*:\s*"([a-zA-Z0-9_.-]+)"/g)].map(match => match[1]);
    const message = String(error.message).replace(/https?:\/\/[^\s'"<>]+/g, '[url]');
    process.stdout.write(`${JSON.stringify({platform, method, realAuthorization: 'failed', codes, message})}\n`);
  } finally {
    if (authenticated && ready) await request('provider_sign_out', scope);
  }
}

try {
  await attachCore();
  await request('plugin_set_developer_mode', { enabled: true });
  const installed = await request('plugin_install_from', {
    request: { path: pluginPath, enable: true, grant: permissions },
  });
  if (browserLogin) {
    await testInteractiveBrowserLogin(browserLogin[1], browserLogin[2]);
  } else if (prepareUi) {
    const scope={providerId:'org.yaqmc.providers.music.netease',profileId:'default'};
    const result=await request('provider_search',{...scope,query:'屋顶',kind:'song',page:1,limit:20});
    const song=result.items.find(item=>item.provider.trackId==='5257138')??result.items[0];
    if(!song)throw new Error('UI sample was missing');
    await request('player_hydrate_queue',{tracks:[song]});
    await request('player_set_volume',{volume:0});
    await request('app_preferences_set',{value:JSON.stringify({version:2,locale:'zh-CN',lyrics:{commentDanmaku:true},system:{closeBehavior:'quit',globalShortcutsEnabled:false,deepLinksEnabled:false,clipboardDeepLinksEnabled:false}})});
    process.stdout.write(`${JSON.stringify({uiProfile:root,prepared:true})}\n`);
  } else if (catalogProbe) {
    const scope = {providerId:'org.yaqmc.providers.music.netease',profileId:'default'};
    const checks = [
      ['search', 'provider_search', {...scope,query:'den',kind:'song',page:1,limit:20}],
      ['restricted-song', 'provider_song', {...scope,id:'19827042'}],
      ['lyrics', 'provider_lyrics', {...scope,id:'5257138'}],
      ['home', 'provider_home', {...scope,refresh:true}],
      ['discover', 'provider_discover', {...scope,refresh:true}],
      ['playlist', 'provider_playlist', {...scope,id:'19723756'}],
      ['artist', 'provider_artist', {...scope,id:'6452'}],
      ['artist-albums', 'provider_artist_catalog', {...scope,id:'6452',kind:'album',page:1,limit:20}],
      ['album', 'provider_album', {...scope,id:'38185'}],
      ...['artist','album','playlist'].map(kind=>[`search-${kind}`,'provider_search',{...scope,query:'周杰伦',kind,page:1,limit:20}]),
    ];
    for(const [name,method,params] of checks) {
      try {
        const value=await request(method,params);
        if (name === 'restricted-song' && (value.availability?.status !== 'entitlement-required' || value.availability?.requiredTier !== 'music-vip' || value.playbackCapability?.status === 'full')) {
          throw new Error('The reported VIP track did not retain its catalog billing restriction');
        }
        const summary={name,passed:true,items:value?.items?.length,lyrics:value?.lines?.length,comments:value?.comments?.length,positiveLikeCounts:value?.comments?.filter(item=>item.likedCount>0).length,tracks:value?.tracks?.length,topSongs:value?.topSongs?.length,playlists:value?.recommendedSonglists?.length,charts:value?.charts?.length};
        process.stdout.write(`${JSON.stringify(summary)}\n`);
        if (name === 'restricted-song') process.stdout.write(`${JSON.stringify({state:'billing-metadata',providerId:scope.providerId,trackId:value.id,availability:value.availability,claimsFullPlayback:value.playbackCapability?.status==='full'})}\n`);
      } catch(error) {
        process.exitCode=2;
        process.stdout.write(`${JSON.stringify({name,passed:false,message:String(error.message).replace(/https?:\/\/[^\s'"<>]+/g,'[url]')})}\n`);
      }
    }
    const matches=await request('provider_search',{...scope,query:'千万次想象',kind:'song',page:1,limit:20});
    const sample=matches.items.find(item=>item.title==='千万次想象');
    if(!sample) throw new Error('The reported song was missing from search');
    const reported=await request('provider_lyrics',{...scope,id:sample.id});
    if(!reported?.lines?.length||!reported.comments?.length)throw new Error('The reported song is missing lyrics or comments');
    process.stdout.write(`${JSON.stringify({name:'reported-song',passed:true,lyrics:reported.lines.length,comments:reported.comments.length,positiveLikeCounts:reported.comments.filter(item=>item.likedCount>0).length})}\n`);
  } else if (loginPlatform) {
    await testInteractiveLogin(loginPlatform);
  } else {
  const initialProviders = await assertProviderMode('isolated');
  const preferences = {spotify: 'standard', netease: 'lossless', kugou: 'high'};
  for (const [platform, quality] of Object.entries(preferences)) {
    const scope = {providerId: `org.yaqmc.providers.music.${platform}`, profileId: 'default'};
    const status = await request('provider_set_preferred_quality', {...scope, quality});
    if (status.preferredQuality !== quality || status.providerId !== scope.providerId) {
      throw new Error(`${platform} quality setter did not return the scoped preference`);
    }
  }
  await restartCore();
  const restartedProviders = await assertProviderMode('isolated');
  const restoredPreferences = {};
  for (const [platform, quality] of Object.entries(preferences)) {
    const scope = {providerId: `org.yaqmc.providers.music.${platform}`, profileId: 'default'};
    const status = await request('provider_status', scope);
    if (status.preferredQuality !== quality) throw new Error(`${platform} quality preference did not survive Core restart`);
    restoredPreferences[platform] = status.preferredQuality;
    await request('provider_set_preferred_quality', {...scope, quality: 'automatic'});
  }
  let playbackEnvironment = {available: true};
  try {
    await request('player_set_volume', { volume: 0 });
  } catch (error) {
    if (!error.message.includes('No usable audio output device is available')) throw error;
    playbackAvailable = false;
    const outputs = await request('audio_output_devices');
    playbackEnvironment = {
      available: false,
      reportedDeviceCount: Array.isArray(outputs) ? outputs.length : outputs.devices?.length,
      error: 'audio-output-unavailable',
    };
    process.exitCode = 2;
  }
  const result = {
    uiProfile: uiProfile ? root : undefined,
    plugin: { id: installed.id, version: installed.version, status: installed.status },
    restoredPreferences,
    initialProviders,
    restartedProviders,
    playbackEnvironment,
    searchPages: {
      netease: await testSearchPages('org.yaqmc.providers.music.netease', '屋顶'),
      kugou: await testSearchPages('org.yaqmc.providers.music.kugou', 'Monkeys Spinning Monkeys'),
    },
    netease: await testPlatform('org.yaqmc.providers.music.netease', '屋顶', true),
    kugou: await testPlatform('org.yaqmc.providers.music.kugou', '晴天'),
    kugouChildren: await testPlatform('org.yaqmc.providers.music.kugou', '两只老虎'),
    kugouInstrumental: await testPlatform('org.yaqmc.providers.music.kugou', 'Monkeys Spinning Monkeys', true),
    neteaseQr: await testQr('org.yaqmc.providers.music.netease'),
    kugouQr: await testQr('org.yaqmc.providers.music.kugou'),
    spotifyPreparation: await testSpotifyPreparation(),
    browserPreparation: await testBrowserPreparation(),
  };
  const isolated = await assertProviderMode('isolated');
  await request('plugin_settings_set', {request:{id:installed.id,values:{mode:'aggregate'}}});
  const aggregate = await assertProviderMode('aggregate');
  await restartCore();
  const aggregateAfterRestart = await assertProviderMode('aggregate');
  await request('player_set_volume', { volume: 0 }).catch(error => {
    if (playbackAvailable) throw error;
  });
  result.aggregatePlayback = {
    netease: await testPlatform('org.yaqmc.providers.music.aggregate', '屋顶', true, song => {
      const route = sourceRoute(song);
      return route?.providerId === 'org.yaqmc.providers.music.netease' && route.trackId === '5257138';
    }),
    kugou: await testPlatform('org.yaqmc.providers.music.aggregate', 'Monkeys Spinning Monkeys', true, song => {
      const route = sourceRoute(song);
      return route?.providerId === 'org.yaqmc.providers.music.kugou' && route.trackId === 'F09B7FAE3D1A3860C10738EBB33DCBBA';
    }),
  };
  await request('plugin_settings_set', {request:{id:installed.id,values:{mode:'isolated'}}});
  const isolatedAfterSwitch = await assertProviderMode('isolated');
  await restartCore();
  const isolatedAfterRestart = await assertProviderMode('isolated');
  result.providers = {isolated,aggregate,aggregateAfterRestart,isolatedAfterSwitch,isolatedAfterRestart};
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  if (playbackAvailable) {
    const playableSamples = {
      netease: result.netease,
      kugou: result.kugouInstrumental,
      aggregateNetease: result.aggregatePlayback.netease,
      aggregateKugou: result.aggregatePlayback.kugou,
    };
    for (const [name, sample] of Object.entries(playableSamples)) {
      if (sample.playbackError || sample.qualitySwitchError || !(sample.advancedPositionMs > 0) || !sample.qualitySwitch) {
        throw new Error(`${name} expected-playable probe failed: ${sample.playbackError ?? sample.qualitySwitchError ?? 'missing advancing clock or quality selection'}`);
      }
    }
  }
  }
} catch (error) {
  for (const line of stderr.split('\n').filter(line => line.includes('plugin.component'))) {
    process.stderr.write(`${line}\n`);
  }
  throw error;
} finally {
  await stopCore();
  if (!uiProfile) {
    if (dirname(resolve(root)) !== temporaryRoot || !basename(root).startsWith('yaqmc-provider-smoke-')) {
      throw new Error('Refusing to clean a directory outside the temporary test root');
    }
    try {
      rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    } catch (error) {
      // Electron may retain a Windows handle to its profile until app.exit().
      // Report the exact owned test directory for cleanup after process exit.
      if (!electronApp || !['EPERM', 'EBUSY'].includes(error.code)) throw error;
      process.stdout.write(`${JSON.stringify({cleanupPending: basename(root), reason: error.code})}\n`);
    }
  }
  if (electronApp) electronApp.exit(process.exitCode ?? 0);
}
