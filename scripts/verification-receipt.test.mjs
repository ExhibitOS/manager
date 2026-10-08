// SPDX-License-Identifier: Apache-2.0
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,rm,symlink,link,rename} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {collectReceipt} from './verification-receipt.mjs';
async function fixture(t){
 const root=await mkdtemp(join(tmpdir(),'exhibitos-receipt-')),binary=join(root,'synthetic.exe'),bundle=join(root,'bundle');t.after(()=>rm(root,{recursive:true,force:true}));await mkdir(bundle);
 await writeFile(binary,'synthetic executable');await writeFile(join(bundle,'runtime.tar'),'synthetic archive');
 const sha256=createHash('sha256').update('synthetic archive').digest('hex');
 await writeFile(join(bundle,'manifest.json'),JSON.stringify({images:[{archive:{path:'runtime.tar',bytes:17,sha256}}]}));return {binary,bundle};
}
test('read-only inventory records exact bytes and leaves files unchanged',async(t)=>{
 const input=await fixture(t),before=await readFile(join(input.bundle,'manifest.json'));const receipt=await collectReceipt(input);
 assert.equal(receipt.archives[0].bytes,17);assert.equal(receipt.binary.bytes,20);assert.match(receipt.sourceCommit,/^[a-f0-9]{40}$/);assert.ok(!JSON.stringify(receipt).includes(input.bundle));
 assert.deepEqual(await readFile(join(input.bundle,'manifest.json')),before);
});
test('changed archive bytes refuse instead of reporting successful verification',async(t)=>{
 const input=await fixture(t);await writeFile(join(input.bundle,'runtime.tar'),'changed bytes');await assert.rejects(collectReceipt(input),/RECEIPT_ARCHIVE_MISMATCH/);
});
test('manifest path escape refuses without reading outside bundle',async(t)=>{
 const input=await fixture(t);await writeFile(join(input.bundle,'manifest.json'),JSON.stringify({images:[{archive:{path:'../private.tar',bytes:17,sha256:'a'.repeat(64)}}]}));await assert.rejects(collectReceipt(input),/RECEIPT_ARCHIVE_INVALID/);
});

test('manifest quota refuses before hashing oversized input',async(t)=>{
 const input=await fixture(t);await writeFile(join(input.bundle,'manifest.json'),Buffer.alloc(1024*1024+1));
 await assert.rejects(collectReceipt(input),/RECEIPT_MANIFEST_TOO_LARGE/);
});
test('aliased archive names refuse instead of counting the same archive twice',async(t)=>{
 const input=await fixture(t),manifest=JSON.parse(await readFile(join(input.bundle,'manifest.json')));
 manifest.images.push(manifest.images[0]);await writeFile(join(input.bundle,'manifest.json'),JSON.stringify(manifest));
 await assert.rejects(collectReceipt(input),/RECEIPT_ARCHIVE_INVALID/);
});
test('hardlinked input refuses without changing either name',async(t)=>{
 const input=await fixture(t),alias=join(input.bundle,'alias.tar');await link(join(input.bundle,'runtime.tar'),alias);
 await assert.rejects(collectReceipt(input),/RECEIPT_FILE_UNSAFE/);
 assert.equal(await readFile(alias,'utf8'),'synthetic archive');
});
test('symlink input refuses without following it',async(t)=>{
 const input=await fixture(t),alias=join(input.bundle,'alias.tar');await symlink(join(input.bundle,'runtime.tar'),alias);
 const manifest=JSON.parse(await readFile(join(input.bundle,'manifest.json')));manifest.images[0].archive.path='alias.tar';
 await writeFile(join(input.bundle,'manifest.json'),JSON.stringify(manifest));await assert.rejects(collectReceipt(input),/RECEIPT_FILE_UNSAFE/);
});
test('manifest rewrites during actual streamed inventory cannot return success',async(t)=>{
 const input=await fixture(t),bytes=Buffer.alloc(16*1024*1024,17),manifestPath=join(input.bundle,'manifest.json');
 await writeFile(join(input.bundle,'runtime.tar'),bytes);
 const original=JSON.stringify({images:[{archive:{path:'runtime.tar',bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')}}]});
 await writeFile(manifestPath,original);
 // Rewrite identical bytes: hashes alone still match, but the inventory must
 // reject the observed modification. Continue until the reader has finished.
 let stopped=false,writes=0;
 const writer=(async()=>{while(!stopped){const pending=join(input.bundle,'manifest-rewrite.pending');await writeFile(pending,original);await rename(pending,manifestPath);writes++;await new Promise(resolve=>setTimeout(resolve,1));}})();
 try {await assert.rejects(collectReceipt(input),/RECEIPT_FILE_CHANGED/);}
 finally {stopped=true;await writer;}
 assert.ok(writes>0);assert.equal(await readFile(manifestPath,'utf8'),original);
});

test('source commit changed during inventory refuses an old source label',async(t)=>{
 const input=await fixture(t),source=join(input.bundle,'source');await mkdir(source);
 const git=args=>execFileSync('git',args,{cwd:source,stdio:'pipe',windowsHide:true});
 git(['init']);await writeFile(join(source,'tracked.txt'),'initial');git(['add','tracked.txt']);
 const commit=['-c','user.name=Synthetic receipt test','-c','user.email=synthetic@example.invalid','-c','commit.gpgSign=false','commit','-m'];
 git([...commit,'initial']);
 // Scheduling before collectReceipt means the first synchronous git snapshot
 // completes before this actual commit runs at an async file-read boundary.
 let changed=false;
 const writer=new Promise((resolve,reject)=>setTimeout(async()=>{try{
   await writeFile(join(source,'tracked.txt'),'changed');git(['add','tracked.txt']);git([...commit,'changed']);changed=true;resolve();
 }catch(error){reject(error);}},1));
 try {await assert.rejects(collectReceipt({...input,source}),/RECEIPT_SOURCE_CHANGED/);}
 finally {await writer;}
 assert.equal(changed,true);
});
