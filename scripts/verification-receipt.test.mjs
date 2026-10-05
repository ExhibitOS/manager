// SPDX-License-Identifier: Apache-2.0
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {collectReceipt} from './verification-receipt.mjs';
async function fixture(){
 const root=await mkdtemp(join(tmpdir(),'exhibitos-receipt-')),binary=join(root,'synthetic.exe'),bundle=join(root,'bundle');await mkdir(bundle);
 await writeFile(binary,'synthetic executable');await writeFile(join(bundle,'runtime.tar'),'synthetic archive');
 const sha256=createHash('sha256').update('synthetic archive').digest('hex');
 await writeFile(join(bundle,'manifest.json'),JSON.stringify({images:[{archive:{path:'runtime.tar',bytes:17,sha256}}]}));return {binary,bundle};
}
test('read-only inventory records exact bytes and leaves files unchanged',async()=>{
 const input=await fixture(),before=await readFile(join(input.bundle,'manifest.json'));const receipt=await collectReceipt(input);
 assert.equal(receipt.archives[0].bytes,17);assert.equal(receipt.binary.bytes,20);assert.match(receipt.sourceCommit,/^[a-f0-9]{40}$/);assert.ok(!JSON.stringify(receipt).includes(input.bundle));
 assert.deepEqual(await readFile(join(input.bundle,'manifest.json')),before);
});
test('changed archive bytes refuse instead of reporting successful verification',async()=>{
 const input=await fixture();await writeFile(join(input.bundle,'runtime.tar'),'changed bytes');await assert.rejects(collectReceipt(input),/RECEIPT_ARCHIVE_MISMATCH/);
});
test('manifest path escape refuses without reading outside bundle',async()=>{
 const input=await fixture();await writeFile(join(input.bundle,'manifest.json'),JSON.stringify({images:[{archive:{path:'../private.tar',bytes:17,sha256:'a'.repeat(64)}}]}));await assert.rejects(collectReceipt(input),/RECEIPT_ARCHIVE_INVALID/);
});
