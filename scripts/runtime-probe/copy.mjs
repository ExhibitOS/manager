// SPDX-License-Identifier: Apache-2.0
// Fresh bounded probe copies only. Never adopts or overwrites a destination.
import { constants } from 'node:fs';
import { open, lstat, realpath, readdir, mkdir, chmod, chown, statfs } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, isAbsolute } from 'node:path';
const fail = () => { throw Error('RUNTIME_PROBE_COPY_REFUSED'); };
const fields = ['dev', 'ino', 'mode', 'uid', 'gid', 'nlink', 'size', 'mtimeNs', 'ctimeNs'];
const id = stat => fields.map(k => stat[k].toString()).join(':');
export async function inventory(root, { maxBytes, maxEntries = 200000 }) {
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 1 || !Number.isSafeInteger(maxEntries) || maxEntries < 1 || maxEntries > 200000) fail();
  if (await realpath(root) !== root || !(await lstat(root)).isDirectory()) fail();
  let total = 0; const rows = [];
  async function walk(rel, depth) {
    if (depth > 64 || rows.length >= maxEntries) fail();
    const path = root + (rel ? '/' + rel : ''), before = await lstat(path, { bigint: true });
    if (before.mode & 0o7000n || await realpath(path) !== path) fail();
    if (before.isDirectory()) {
      rows.push({ path: rel, kind: 'directory', mode: Number(before.mode & 0o777n), identity: id(before) });
      for (const name of (await readdir(path)).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)))) {
        if (!name || ['.', '..'].includes(name) || /[\/\0]/.test(name)) fail();
        await walk(rel ? rel + '/' + name : name, depth + 1);
      }
      if (id(await lstat(path, { bigint: true })) !== id(before)) fail();
    } else {
      if (!before.isFile() || before.nlink !== 1n || before.size > BigInt(maxBytes)) fail();
      total += Number(before.size); if (total > maxBytes) fail();
      const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK), hash = createHash('sha256'), buffer = Buffer.alloc(65536); let bytes = 0;
      try {
        if (id(await file.stat({ bigint: true })) !== id(before)) fail();
        for (;;) { const n = (await file.read(buffer, 0, buffer.length, null)).bytesRead; if (!n) break; bytes += n; if (bytes > Number(before.size)) fail(); hash.update(buffer.subarray(0, n)); buffer.fill(0); }
        if (bytes !== Number(before.size) || id(await file.stat({ bigint: true })) !== id(before) || id(await lstat(path, { bigint: true })) !== id(before)) fail();
        rows.push({ path: rel, kind: 'file', mode: Number(before.mode & 0o777n), bytes, sha256: hash.digest('hex'), identity: id(before) });
      } finally { buffer.fill(0); await file.close(); }
    }
  }
  await walk('', 0); return { rows, bytes: total };
}
export const content = snapshot => snapshot.rows.map(({ identity, ...row }) => row);
export async function copy(source, destination, limits, owner) {
  if (!isAbsolute(source) || !isAbsolute(destination) || await realpath(dirname(destination)) !== dirname(destination) || destination === source || destination.startsWith(source + '/') || source.startsWith(destination + '/') || !owner || !Number.isInteger(owner.uid) || owner.uid < 0 || !Number.isInteger(owner.gid) || owner.gid < 0) fail();
  const original = await inventory(source, limits);
  const space = await statfs(dirname(destination), { bigint: true }); if (space.bavail * space.bsize < BigInt(original.bytes) + 32n * 1024n * 1024n) fail();
  await mkdir(destination, { mode: 0o700 }); // EEXIST is an unconditional refusal, including empty folders.
  for (const row of original.rows) {
    if (!row.path) continue;
    const target = destination + '/' + row.path;
    if (row.kind === 'directory') { await mkdir(target, { mode: 0o700 }); continue; }
    const input = await open(source + '/' + row.path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK); let output;
    const hash = createHash('sha256'), buffer = Buffer.alloc(65536); let bytes = 0;
    try {
      if (id(await input.stat({ bigint: true })) !== row.identity) fail();
      output = await open(target, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
      for (;;) { const n = (await input.read(buffer, 0, buffer.length, null)).bytesRead; if (!n) break; bytes += n; if (bytes > row.bytes) fail(); hash.update(buffer.subarray(0, n)); let at = 0; while (at < n) { const written = (await output.write(buffer, at, n - at, null)).bytesWritten; if (!written) fail(); at += written; } buffer.fill(0); }
      if (bytes !== row.bytes || hash.digest('hex') !== row.sha256 || id(await input.stat({ bigint: true })) !== row.identity || id(await lstat(source + '/' + row.path, { bigint: true })) !== row.identity) fail();
      await output.chown(owner.uid, owner.gid); await output.chmod(row.mode); await output.sync();
    } finally { buffer.fill(0); await output?.close(); await input.close(); }
  }
  for (const row of original.rows.filter(r => r.kind === 'directory').reverse()) { const path = destination + (row.path ? '/' + row.path : ''); await chown(path, owner.uid, owner.gid); await chmod(path, row.mode); }
  const after = await inventory(source, limits), copied = await inventory(destination, limits);
  if (JSON.stringify(after) !== JSON.stringify(original) || JSON.stringify(content(copied)) !== JSON.stringify(content(original))) fail();
  return original;
}
