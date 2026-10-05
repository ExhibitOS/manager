// SPDX-License-Identifier: Apache-2.0
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink, link, lstat, chmod, readdir, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { copy, inventory, content } from './copy.mjs';
const limits = { maxBytes: 1024 * 1024, maxEntries: 64 }, owner = { uid: process.getuid(), gid: process.getgid() };
async function fixture(work) { const base = await realpath(await mkdtemp(join(tmpdir(), 'exhibitos-probe-copy-'))); try { const source = join(base, 'source'); await mkdir(source, { mode: 0o700 }); await work(base, source); } finally { await rm(base, { recursive: true }); } }
test('fresh copy preserves exact content and mode without changing original identity', () => fixture(async (base, source) => {
  await mkdir(join(source, 'nested'), { mode: 0o700 }); await writeFile(join(source, 'nested', 'private'), 'synthetic bytes', { mode: 0o600 });
  const before = await inventory(source, limits), target = join(base, 'target'); await copy(source, target, limits, owner);
  assert.deepEqual(await inventory(source, limits), before); assert.deepEqual(content(await inventory(target, limits)), content(before)); assert.equal((await lstat(join(target, 'nested', 'private'))).mode & 0o777, 0o600);
}));
test('existing destination is refused, with original bytes preserved', () => fixture(async (base, source) => {
  await writeFile(join(source, 'one'), 'source'); const target = join(base, 'target'); await mkdir(target); await writeFile(join(target, 'keep'), 'keep');
  await assert.rejects(copy(source, target, limits, owner)); assert.equal(await readFile(join(target, 'keep'), 'utf8'), 'keep'); assert.equal(await readFile(join(source, 'one'), 'utf8'), 'source');
}));
test('symlink hardlink and privileged mode refuse before destination creation', () => fixture(async (base, source) => {
  const one = join(source, 'one'); await writeFile(one, 'one', { mode: 0o600 }); await symlink(one, join(source, 'alias')); await assert.rejects(copy(source, join(base, 'link-target'), limits, owner)); await rm(join(source, 'alias'));
  await link(one, join(source, 'hard')); await assert.rejects(copy(source, join(base, 'hard-target'), limits, owner)); await rm(join(source, 'hard'));
  await chmod(one, 0o4600); await assert.rejects(copy(source, join(base, 'mode-target'), limits, owner)); assert.deepEqual((await readdir(base)).sort(), ['source']);
}));
test('byte/entry/depth/parent alias limits refuse without copying or rewriting', () => fixture(async (base, source) => {
  await writeFile(join(source, 'bytes'), 'too large'); await assert.rejects(copy(source, join(base, 'large'), { ...limits, maxBytes: 1 }, owner)); await assert.rejects(copy(source, join(base, 'entries'), { ...limits, maxEntries: 1 }, owner));
  const alias = join(base, 'alias'); await symlink(base, alias); await assert.rejects(copy(source, join(alias, 'target'), limits, owner)); assert.equal(await readFile(join(source, 'bytes'), 'utf8'), 'too large');
}));
test('deep input is refused before creating a destination', () => fixture(async (base, source) => {
  let path = source;
  for (let i = 0; i < 65; i++) { path = join(path, 'd'); await mkdir(path, { mode: 0o700 }); }
  await assert.rejects(copy(source, join(base, 'target'), { ...limits, maxEntries: 200 }, owner)); assert.deepEqual(await readdir(base), ['source']);
}));
