// SPDX-License-Identifier: Apache-2.0
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {inspectionRequest} from './native-legacy-inspect.mjs';
const request=()=>({backupId:'9a2bbc60-55af-469b-b6bb-d72a998d4e0d',candidateId:'11111111-2222-4333-8444-555555555555',manifestPath:'/private/tmp/synthetic-restored/manifest.json',manifestSha256:'a'.repeat(64),payloadSha256:'b'.repeat(64),systemIdentifier:'18446744073709551615'});
test('inspection request cannot select code, SQL, a target catalog or a previous receipt',()=>{
 const input=request(),admitted=inspectionRequest(input);input.systemIdentifier='1';
 assert.equal(admitted.systemIdentifier,'18446744073709551615');
 for(const name of ['script','migrationDirectory','targetMigrations','targetSchemaSha256','inspectionReceipt','operation'])assert.throws(()=>inspectionRequest({...request(),[name]:'caller supplied'}),/NATIVE_LEGACY_INSPECTION_FAILED/);
 assert.throws(()=>inspectionRequest({...request(),manifestPath:'../manifest.json'}),/NATIVE_LEGACY_INSPECTION_FAILED/);
});
test('inspection binding rejects malformed hashes, IDs and noncanonical or overflowing database identity',()=>{
 for(const id of ['0','01','-1','+1','18446744073709551616','1\n',1,null])assert.throws(()=>inspectionRequest({...request(),systemIdentifier:id}),/NATIVE_LEGACY_INSPECTION_FAILED/);
 for(const field of ['backupId','candidateId','manifestSha256','payloadSha256'])assert.throws(()=>inspectionRequest({...request(),[field]:'invalid'}),/NATIVE_LEGACY_INSPECTION_FAILED/);
 for(const value of [null,[],{},'request'])assert.throws(()=>inspectionRequest(value),/NATIVE_LEGACY_INSPECTION_FAILED/);
});
