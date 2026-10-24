// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {generateKeyPairSync,createPublicKey,sign} from 'node:crypto';
import {freezeCanonical,sha256,verifyFreezeSignature} from '../vendor/platform/freeze-signature.mjs';

test('adopted public verifier preserves original authority and signed canonical bytes',()=>{
 // In-memory, disposable unit-test material only; no file or credential store.
 const {privateKey}=generateKeyPairSync('ed25519');
 const publicKey=createPublicKey(privateKey).export({type:'spki',format:'der'});
 const authority={origin:'http://127.0.0.1:13200',keyId:sha256(publicKey),publicKey:publicKey.toString('base64')};
 const manifest={authority,kind:'unit-test-only',nested:{z:1,a:[2,1]}};
 const bytes=Buffer.from(freezeCanonical(manifest));
 const row={manifest,manifest_sha256:sha256(bytes),signature:sign(null,bytes,privateKey).toString('base64')};
 assert.doesNotThrow(()=>verifyFreezeSignature(row,authority,privateKey));
 for(const field of ['origin','keyId','publicKey']){
  assert.throws(()=>verifyFreezeSignature(row,{...authority,[field]:'changed'},privateKey),error=>error.status===403&&error.code==='FREEZE_AUTHORITY_CHANGED');
 }
 assert.throws(()=>verifyFreezeSignature({...row,manifest_sha256:'0'.repeat(64)},authority,privateKey),error=>error.status===409&&error.code==='FREEZE_INTEGRITY');
 assert.throws(()=>verifyFreezeSignature({...row,manifest:{...manifest,nested:{z:2,a:[2,1]}}},authority,privateKey),error=>error.code==='FREEZE_INTEGRITY');
 assert.throws(()=>verifyFreezeSignature({...row,signature:Buffer.alloc(64).toString('base64')},authority,privateKey),error=>error.code==='FREEZE_INTEGRITY');
});
test('adopted canonicalizer retains array order and refuses non-JSON signing values',()=>{
 assert.equal(freezeCanonical({z:[2,1],a:1}),'{"a":1,"z":[2,1]}');
 for(const value of [undefined,NaN,Infinity])assert.throws(()=>freezeCanonical(value),/FREEZE_INVALID/);
});
