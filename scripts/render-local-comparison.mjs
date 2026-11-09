// SPDX-License-Identifier: Apache-2.0
import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { mkdtemp, open, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';

const maxRequestBytes = 1_100_000;

async function comparisonRequest(path) {
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const info = await file.stat();
    if (!info.isFile() || info.size < 1 || info.size > maxRequestBytes) {
      throw Error('AGENT_REQUEST');
    }
    const bytes = Buffer.alloc(info.size + 1);
    let length = 0;
    while (length < bytes.length) {
      const { bytesRead } = await file.read(bytes, length, bytes.length - length, length);
      if (!bytesRead) break;
      length += bytesRead;
    }
    if (length !== info.size) throw Error('AGENT_REQUEST');
    const request = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, length)));
    if (request?.version !== 1 || request.operation !== 'raw-asset-comparison') {
      throw Error('AGENT_REQUEST');
    }
    return bytes.subarray(0, length);
  } catch {
    throw Error('AGENT_REQUEST');
  } finally {
    await file.close();
  }
}

async function main() {
  const { values } = parseArgs({ options: {
    'platform-root': { type: 'string' },
    input: { type: 'string' },
    output: { type: 'string' },
  } });
  const { 'platform-root': platform, input, output } = values;
  if (![platform, input, output].every(path => path && isAbsolute(path) && resolve(path) === path)) {
    throw Error('AGENT_USAGE');
  }
  const entry = join(platform, 'scripts/private-agent-renderer.mjs');
  if (await realpath(entry) !== entry || !(await stat(entry)).isFile()) {
    throw Error('AGENT_USAGE');
  }

  let scratch, child, cancelled = false;
  function cancel() {
    if (cancelled) return;
    cancelled = true;
    if (child?.stdin.writable) child.stdin.end('{"cancel":true}\n');
  }
  process.on('SIGINT', cancel);
  process.on('SIGTERM', cancel);
  try {
    const request = await comparisonRequest(input);
    scratch = await mkdtemp(join(await realpath(tmpdir()), 'exhibitos-comparison-'));
    const snapshot = join(scratch, 'request.json');
    await writeFile(snapshot, request, { flag: 'wx', mode: 0o600 });
    if (cancelled) throw Error('AGENT_CANCELLED');
    // Keep the existing renderer's owner pipe open until it closes or we cancel.
    // Only the private request snapshot reaches the renderer.
    child = spawn(process.execPath, [entry, '--input', snapshot, '--output', output], {
      cwd: platform,
      stdio: ['pipe', 'inherit', 'inherit'],
    });
    child.stdin.on('error', () => {});
    return await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('close', status => resolve(status ?? 1));
    });
  } finally {
    process.off('SIGINT', cancel);
    process.off('SIGTERM', cancel);
    if (scratch) await rm(scratch, { recursive: true });
  }
}

try {
  process.exitCode = await main();
} catch (error) {
  const code = ['AGENT_USAGE', 'AGENT_REQUEST', 'AGENT_CANCELLED'].includes(error.message)
    ? error.message
    : error.code?.startsWith('ERR_PARSE_ARGS_') ? 'AGENT_USAGE' : 'AGENT_RENDER_FAILED';
  console.log(JSON.stringify({ version: 1, status: 'failed', code }));
  process.exitCode = 1;
}
