// SPDX-License-Identifier: Apache-2.0
// Injected only into an isolated real runtime image; never mounts writable originals.
import { request as httpRequest } from 'node:http';
// The trusted host embeds copy.mjs above this body; no arbitrary code is read from a mounted source.
const blobLimits = { maxBytes: 128 * 1024 * 1024 }, configLimits = { maxBytes: 8 * 1024 * 1024, maxEntries: 512 };
const safeError = code => { throw Error(code); };
async function boundedJson(url, options) {
  const response = await fetch(url, { ...options, signal: AbortSignal.timeout(5000) });
  if (!response.ok || Number(response.headers.get('content-length') ?? 0) > 65536) safeError('RUNTIME_PROBE_RESPONSE_INVALID');
  let size = 0; const chunks = [];
  for await (const chunk of response.body) { size += chunk.length; if (size > 65536) safeError('RUNTIME_PROBE_RESPONSE_LIMIT'); chunks.push(chunk); }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
let runtime, result, failure;
try {
  const envFile = await open('/runtime.env', constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK); let raw;
  try { const stat = await envFile.stat(); if (!stat.isFile() || stat.size > 8192 || stat.nlink !== 1 || stat.mode & 0o077) safeError('RUNTIME_PROBE_ENV_INVALID'); raw = await envFile.readFile('utf8'); }
  finally { await envFile.close(); }
  const environment = {};
  for (const line of raw.trimEnd().split('\n')) { const at = line.indexOf('='); const name = line.slice(0, at), value = line.slice(at + 1); if (at < 1 || !/^[A-Z_]+$/.test(name) || Object.hasOwn(environment, name) || /[\0\r]/.test(value)) safeError('RUNTIME_PROBE_ENV_INVALID'); environment[name] = value; }
  if (Object.keys(environment).sort().join(',') !== 'ADMIN_PASSWORD,ADMIN_SUBJECT,DATABASE_URL,EXHIBITOS_PORT,POSTGRES_PASSWORD,TENANT_ID') safeError('RUNTIME_PROBE_ENV_INVALID');
  const ready = await boundedJson('http://127.0.0.1:5433/ready'); if (ready.ready !== true) safeError('RUNTIME_PROBE_DATABASE_UNAVAILABLE');
  const blobs = await copy('/source-blobs', '/probe/blobs', blobLimits, { uid: 1000, gid: 1000 }), configuration = await copy('/source-config', '/probe/config', configLimits, { uid: 1000, gid: 1000 });
  const url = new URL(environment.DATABASE_URL); if (url.protocol !== 'postgresql:' || url.hostname !== 'database' || url.port !== '5432' || url.pathname !== '/exhibitos' || url.search || url.hash) safeError('RUNTIME_PROBE_ENV_INVALID'); url.hostname = '127.0.0.1';
  environment.DATABASE_URL = url.href; environment.BLOB_ROOT = '/probe/blobs'; environment.CONFIG_ROOT = '/probe/config';
  await chown('/probe', 1000, 1000); await chmod('/probe', 0o700);
  process.setgid(1000); process.setuid(1000); if (process.getuid() !== 1000 || process.getgid() !== 1000) safeError('RUNTIME_PROBE_UID_INVALID');
  const { startLocalRuntime } = await import('/opt/exhibitos/scripts/local-runtime.mjs');
  runtime = await startLocalRuntime(environment);
  const health = await boundedJson('http://127.0.0.1:3000/api/v1/health');
  if (health.status !== 'ok' || health.service !== 'exhibitos-api' || health.version !== '0.1.0') safeError('RUNTIME_PROBE_HEALTH_INVALID');
  const readiness = [];
  for (let i = 0; i < 3; i++) { const observed = await boundedJson('http://127.0.0.1:3000/api/v1/readiness'); if (observed.ready !== true || observed.protocolVersion !== '1' || observed.services?.length !== 5 || observed.services.some(s => s.status !== 'ready')) safeError('RUNTIME_PROBE_READINESS_INVALID'); readiness.push(observed); await delay(250); }
  const webResult = await new Promise((resolve, reject) => {
    const request = httpRequest({ host: '127.0.0.1', port: 8080, path: '/', method: 'GET', headers: { Host: new URL(runtime.origin).host }, timeout: 5000 }, response => {
      if (response.statusCode !== 200 || !response.headers['content-type']?.startsWith('text/html')) { response.resume(); reject(Error('RUNTIME_PROBE_WEB_INVALID')); return; }
      let bytes = 0; const hash = createHash('sha256');
      response.on('data', chunk => { bytes += chunk.length; if (bytes > 16 * 1024 * 1024) { response.destroy(); reject(Error('RUNTIME_PROBE_WEB_INVALID')); } else hash.update(chunk); });
      response.once('error', () => reject(Error('RUNTIME_PROBE_WEB_INVALID'))); response.once('end', () => bytes ? resolve({ bytes, sha256: hash.digest('hex') }) : reject(Error('RUNTIME_PROBE_WEB_INVALID')));
    });
    request.once('timeout', () => { request.destroy(); reject(Error('RUNTIME_PROBE_WEB_INVALID')); }); request.once('error', () => reject(Error('RUNTIME_PROBE_WEB_INVALID'))); request.end();
  });
  await runtime.close(); runtime = null;
  for (const [path, original, limits] of [['/probe/blobs', blobs, blobLimits], ['/probe/config', configuration, configLimits]]) { const current = await inventory(path, limits); if (JSON.stringify(content(current)) !== JSON.stringify(content(original))) safeError('RUNTIME_PROBE_FILES_CHANGED'); }
  const logical = await boundedJson('http://127.0.0.1:5433/finish', { method: 'POST' });
  if (logical.currentInventoryVerified !== true || logical.preflightVerified !== false || logical.updateExecuted !== false) safeError('RUNTIME_PROBE_INVENTORY_INVALID');
  result = { health, readiness, web: webResult, copiedBlobBytes: blobs.bytes, copiedConfigurationBytes: configuration.bytes, logical, uid: 1000, originalMountsReadOnly: true, preflightVerified: false, updateExecuted: false };
} catch (error) { failure = /^[A-Z][A-Z0-9_]{0,79}$/.test(error?.message ?? '') ? error.message : 'RUNTIME_PROBE_TARGET_FAILED'; }
finally { if (runtime) { try { await runtime.close(); } catch { failure = 'RUNTIME_PROBE_CLOSE_FAILED'; } } }
if (failure) { console.error(failure); process.exitCode = 1; } else console.log(JSON.stringify(result));
