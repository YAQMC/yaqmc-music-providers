const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const { fork, spawn } = require('node:child_process');

process.chdir(__dirname);
const children = new Set();
let stopping = false;

function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  for (const child of children) child.kill();
  process.exitCode = code;
}

function own(child, name) {
  children.add(child);
  child.on('error', () => {
    console.error(`${name} could not start.`);
    stop(1);
  });
  child.on('exit', () => {
    children.delete(child);
    if (!stopping) {
      console.error(`${name} stopped. Restart the local services.`);
      stop(1);
    }
  });
  return child;
}

async function checkPort(port) {
  await new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', () => reject(new Error(`Port ${port} is in use. Close the earlier local services first.`)));
    probe.listen(port, '127.0.0.1', () => probe.close(resolve));
  });
}

async function startApi(platform, env) {
  const child = own(fork(path.join(__dirname, 'api-worker.cjs'), [platform], {
    env, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe', 'ipc'],
  }), platform);
  child.stderr.on('data', () => {});
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${platform} startup timed out.`)), 30000);
    child.once('message', () => { clearTimeout(timer); resolve(); });
    child.once('exit', () => { clearTimeout(timer); reject(new Error(`${platform} startup failed.`)); });
    child.once('error', () => { clearTimeout(timer); reject(new Error(`${platform} startup failed.`)); });
  });
}

async function main() {
  const backend = path.join(__dirname, 'yaqmc-music-providers.exe');
  if (!fs.existsSync(backend)) throw new Error('Missing companion backend executable. Use the complete Windows test bundle.');
  await Promise.all([43821, 43823, 43824].map(checkPort));
  if (!fs.existsSync('settings.json')) fs.copyFileSync('settings.example.json', 'settings.json');
  const settings = JSON.parse(fs.readFileSync('settings.json', 'utf8'));
  const clientId = process.env.SPOTIFY_CLIENT_ID || settings.spotifyClientId || '';
  if (typeof clientId !== 'string' || typeof settings.spotifyProxy !== 'string') throw new Error('Invalid settings.json');
  if (settings.spotifyProxy && !/^https?:\/\/[^\s]+$/.test(settings.spotifyProxy)) throw new Error('Invalid spotifyProxy URL');
  const config = [
    'dataDir = "data"', 'mode = "isolated"',
    '[listen]', 'host = "127.0.0.1"', 'port = 43821',
    '[spotify]', `clientId = ${JSON.stringify(clientId)}`,
    'redirectUri = "http://127.0.0.1:43821/oauth/spotify/callback"',
    '[netease]', 'baseUrl = "http://127.0.0.1:43823"',
    '[kugou]', 'baseUrl = "http://127.0.0.1:43824"', '',
  ].join('\n');
  fs.writeFileSync('config.toml', config);
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (/^(https?_proxy|all_proxy|netease_api_base_url|kugou_api_base_url)$/i.test(key)) delete env[key];
  }
  env.NO_PROXY = '127.0.0.1,localhost';
  await startApi('netease', env);
  await startApi('kugou', env);
  const backendEnv = { ...env };
  if (settings.spotifyProxy) backendEnv.HTTPS_PROXY = settings.spotifyProxy;
  own(spawn(backend, ['serve', '--config', 'config.toml'], {
    cwd: __dirname, env: backendEnv, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'],
  }), 'Backend').stderr.on('data', () => {});
  for (let attempt = 0; attempt < 50; attempt++) {
    if (stopping) throw new Error('Backend startup failed.');
    try {
      const response = await fetch('http://127.0.0.1:43821/health', { signal: AbortSignal.timeout(1000) });
      const health = await response.json();
      if (health.ok && health.result?.protocolVersion === 1) {
        console.log('Local services ready: NetEase 43823 / KuGou 43824 / Backend 43821.');
        console.log(clientId ? 'Spotify Client ID configured.' : 'Spotify: fill spotifyClientId in runtime/settings.json, then restart.');
        console.log('Keep this window open while using YAQMC. Ctrl+C stops all three services.');
        return;
      }
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  throw new Error('Backend health check failed.');
}

process.on('SIGINT', () => stop());
process.on('SIGTERM', () => stop());
main().catch(error => { console.error(error.message); stop(1); });
