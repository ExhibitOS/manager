// SPDX-License-Identifier: Apache-2.0
// Host injects a compiled-in database-copy script before this body.
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, chmod, chown } from 'node:fs/promises';
import { createServer } from 'node:http';
import { main } from '/opt/exhibitos/scripts/service-backup.mjs';
const delay = ms => new Promise(resolve => { const timer = setTimeout(resolve, ms); timer.unref(); });
const socket = '/tmp/exhibitos-probe-pg'; let pg, server, pgExit, pgEnded = false, finished = false, failure;
const copyResult = spawnSync(process.execPath, ['--input-type=module', '-e', DATABASE_COPY], { encoding: 'utf8', timeout: 270000, maxBuffer: 65536 });
if (copyResult.status !== 0 || copyResult.error) throw Error('RUNTIME_PROBE_COPY_FAILED');
const physical = JSON.parse(copyResult.stdout);
try {
  await mkdir(socket, { mode: 0o700 }); await chmod(socket, 0o700); await chown(socket, 999, 999);
  pg = spawn('/usr/lib/postgresql/18/bin/postgres', ['-D', '/snapshot/18/docker', '-c', 'listen_addresses=127.0.0.1', '-c', 'unix_socket_directories=' + socket, '-c', 'unix_socket_permissions=0700', '-c', 'autovacuum=off', '-c', 'archive_mode=off', '-c', 'shared_preload_libraries=', '-c', 'session_preload_libraries=', '-c', 'local_preload_libraries='], { uid: 999, gid: 999, stdio: 'ignore' });
  pgExit = new Promise(resolve => { pg.once('error', () => { pgEnded = true; resolve(-1); }); pg.once('exit', code => { pgEnded = true; resolve(code); }); });
  let ready = false;
  for (let i = 0; i < 100; i++) { if (pgEnded) throw Error('RUNTIME_PROBE_DATABASE_FAILED'); const result = spawnSync('/usr/lib/postgresql/18/bin/pg_isready', ['-h', socket, '-U', 'exhibitos', '-d', 'exhibitos'], { stdio: 'ignore', timeout: 1000 }); if (result.status === 0) { ready = true; break; } await delay(100); }
  if (!ready) throw Error('RUNTIME_PROBE_DATABASE_FAILED');
  await new Promise((resolve, reject) => {
    server = createServer(async (request, reply) => {
      request.resume(); reply.setHeader('content-type', 'application/json');
      if (request.method === 'GET' && request.url === '/ready') { reply.end('{"ready":true}'); return; }
      if (request.method !== 'POST' || request.url !== '/finish' || finished) { reply.writeHead(403); reply.end('{"code":"RUNTIME_PROBE_REQUEST_DENIED"}'); return; }
      finished = true;
      try {
        const result = await main(['check-restored-inventory', '--manifest-file', '/manifest.json', '--manifest-sha256', process.env.EXHIBITOS_MANIFEST_SHA256, '--quiesced', '--snapshot-system-identifier', physical.systemIdentifier], { DATABASE_URL: 'postgresql://exhibitos@localhost/exhibitos?host=' + encodeURIComponent(socket), BLOB_BACKEND: 'file', BLOB_ROOT: '/blobs' });
        if (!result.currentInventoryVerified || result.preflightVerified || result.updateExecuted) throw Error('RUNTIME_PROBE_INVENTORY_INVALID');
        reply.end(JSON.stringify(result)); resolve();
      } catch { failure = 'RUNTIME_PROBE_INVENTORY_CHANGED'; reply.writeHead(409); reply.end(JSON.stringify({ code: failure })); resolve(); }
    });
    server.requestTimeout = 10000; server.headersTimeout = 10000; server.keepAliveTimeout = 1000; server.listen(5433, '127.0.0.1');
    server.once('error', reject); setTimeout(() => { failure = 'RUNTIME_PROBE_TIMEOUT'; resolve(); }, 300000).unref();
    console.log(JSON.stringify({ ready: true, physical }));
  });
} catch { failure ??= 'RUNTIME_PROBE_DATABASE_FAILED'; }
finally {
  server?.closeAllConnections(); if (server) await new Promise(resolve => server.close(resolve));
  if (pg && !pgEnded) { pg.kill('SIGINT'); const code = await Promise.race([pgExit, delay(20000).then(() => null)]); if (code !== 0) { failure = 'RUNTIME_PROBE_DATABASE_STOP_FAILED'; if (!pgEnded) { pg.kill('SIGKILL'); await pgExit; } } } else if (pg) failure ??= 'RUNTIME_PROBE_DATABASE_STOP_FAILED';
}
if (failure) { console.error(failure); process.exitCode = 1; } else console.log(JSON.stringify({ completed: true, physical, preflightVerified: false, updateExecuted: false }));
