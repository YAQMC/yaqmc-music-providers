// Electron CJS entry point: finish loading the main module before awaiting
// app.whenReady() in the shared ESM test driver.
const { join } = require('node:path');
const { pathToFileURL } = require('node:url');
const { app } = require('electron');

function failed(error) {
  const message = String(error?.message || error).replace(/https?:\/\/[^\s'"<>]+/g, '[url]');
  process.stderr.write(`${JSON.stringify({ state: 'test-driver-failed', code: error?.code, message })}\n`);
  app.exit(2);
}

process.on('uncaughtException', failed);
process.on('unhandledRejection', failed);
setImmediate(() => import(pathToFileURL(join(__dirname, 'test-host-smoke.mjs')).href).catch(failed));
