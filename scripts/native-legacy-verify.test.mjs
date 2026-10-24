// SPDX-License-Identifier: Apache-2.0
// Closed frame/handle tests only; these do not issue Rust authority or prove AES.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash,randomUUID} from 'node:crypto';
import {mkdtempSync,writeFileSync,openSync,closeSync,readFileSync,chmodSync,linkSync,unlinkSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {readHeldKey,closedResult,safeFailure} from './native-legacy-verify.mjs';
const names=['manager-bundle-manifest.json','manager-compose.yaml','manager-runtime.env','manager-installed.json','manager-engine.json','manager-image-inventory.json','freeze-signing-key.json'];
test('held regular key uses explicit offset zero and rejects shared or public files',()=>{
 const root=mkdtempSync(join(tmpdir(),'legacy-key-handle-')),path=join(root,'synthetic-key');let fd;
 try{
  writeFileSync(path,Buffer.alloc(32,7),{mode:0o600});fd=openSync(path,'r');readFileSync(fd);
  const key=readHeldKey(fd);assert.equal(key.length,32);assert(key.every(v=>v===7));key.fill(0);
  chmodSync(path,0o644);assert.throws(()=>readHeldKey(fd),/KEY_HANDLE_INVALID/);chmodSync(path,0o600);
  linkSync(path,join(root,'shared'));assert.throws(()=>readHeldKey(fd),/KEY_HANDLE_INVALID/);unlinkSync(join(root,'shared'));
 }finally{if(fd!==undefined)closeSync(fd);rmSync(root,{recursive:true,force:true});}
});
test('closed result hashes original manifest bytes and excludes private payload fields',()=>{
 const id=randomUUID(),files=names.map((name,i)=>({role:'configuration',path:`files/${String(i).padStart(8,'0')}.gcm`,name,bytes:128,sha256:'a'.repeat(64),plainPath:'/private/never-in-result'}));
 const bytes=Buffer.from(JSON.stringify({id,files})+' \n');
 const verified={files,receipt:{id,status:'complete',manifestSha256:'c'.repeat(64)}};
 const frame=closedResult(verified,bytes,'c'.repeat(64));
 assert.equal(frame.authenticatedManifestSha256,createHash('sha256').update(bytes).digest('hex'));
 assert.notEqual(frame.authenticatedManifestSha256,createHash('sha256').update(JSON.stringify(JSON.parse(bytes))).digest('hex'));
 assert(!JSON.stringify(frame).includes('/private/'));assert.equal(Object.keys(frame.configuration).length,7);
 assert.throws(()=>closedResult(verified,bytes,'d'.repeat(64)),/RESULT_INVALID/);
 assert.throws(()=>closedResult({...verified,files:[...files.slice(1),files[1]]},bytes,'c'.repeat(64)),/RESULT_INVALID/);
});
test('only fixed failures cross the codec boundary',()=>{
 assert.equal(safeFailure(Error('NATIVE_LEGACY_KEY_HANDLE_INVALID')),'NATIVE_LEGACY_KEY_HANDLE_INVALID');
 for(const error of [Error('token=private'),Error('/private/path'),Error('NATIVE_LEGACY_RESULT_INVALID\nSQL'),{code:'ENOENT'}])assert.equal(safeFailure(error),'NATIVE_LEGACY_AUTHENTICATION_FAILED');
});
