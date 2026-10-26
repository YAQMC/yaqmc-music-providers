const fs = require('node:fs');
const path = require('node:path');

async function main() {
  const platform = process.argv[2];
  console.log = console.info = console.warn = console.error = () => {};
  let server;
  if (platform === 'netease') {
    const tokenFile = path.join(require('node:os').tmpdir(), 'anonymous_token');
    if (!fs.existsSync(tokenFile)) fs.writeFileSync(tokenFile, '');
    const { serveNcmApi } = require('@neteasecloudmusicapienhanced/api/server');
    const app = await serveNcmApi({ host: '127.0.0.1', port: 43823, checkVersion: false });
    server = app.server;
  } else if (platform === 'kugou') {
    process.env.HOST = '127.0.0.1';
    process.env.PORT = '43824';
    const { startService } = require('kugoumusicapi/server');
    const app = await startService();
    server = app.service;
  } else {
    throw new Error('Unknown API service');
  }
  server.on('error', () => process.exit(1));
  const ready = () => process.send?.({ ready: platform });
  if (server.listening) ready();
  else server.once('listening', ready);
  process.on('SIGTERM', () => server.close(() => process.exit(0)));
}

main().catch(() => {
  process.stderr.write('Local API startup failed. Check dependencies and port availability.\n');
  process.exit(1);
});
