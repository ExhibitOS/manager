// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseJob,parseStatus,managerError,formatBytes,statusLabel,validVerificationInput,parseVerificationReceipt} from './lifecycle';
const status={installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0,quotaBytes:5368709120},activeJob:null};
describe('Manager lifecycle boundary',()=>{
 it('preserves unknown storage/readiness without claiming success',()=>{const value=parseStatus(status);expect(formatBytes(value.storage.usedBytes)).toBe('측정할 수 없음');expect(value.readiness.ready).toBe(false);expect(statusLabel(value.state)).toBe('설치 전');});
 it('denies malformed state, unsafe numeric counters and fake successful jobs',()=>{for(const value of [{...status,installed:'true'},{...status,storage:{...status.storage,freeBytes:-1}},{...status,readiness:{...status.readiness,ready:'true'}}])expect(()=>parseStatus(value)).toThrow('MANAGER_PROTOCOL');expect(()=>parseJob({id:'synthetic',action:'shell',state:'completed'})).toThrow('MANAGER_PROTOCOL');});
 it('reports persisted failure and guidance without displaying raw exceptions or credentials',()=>{const job=parseJob({id:'synthetic',action:'start',state:'failed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:'PORT_IN_USE',guidance:'다른 앱의 포트 사용을 확인하세요.'});expect(job.state).toBe('failed');expect(managerError(new Error('postgres://private:secret@host/db')).guidance).not.toContain('secret');expect(managerError({code:'PORT_IN_USE',guidance:job.guidance}).code).toBe('PORT_IN_USE');});
});

const image='sha256:'+'a'.repeat(64),verification={id:'12345678-1234-1234-1234-123456789012',operation:'verified',files:68,image,authenticatedManifestSha256:'b'.repeat(64),at:2};
describe('Backup verification native boundary',()=>{
 it('rejects raw key contents, relative paths and mount injection before IPC',()=>{
  expect(validVerificationInput({image,keyPath:'/private/key',sourcePath:'/private/archive'})).toBe(true);
  for(const input of [{image:'image:latest',keyPath:'/private/key',sourcePath:'/private/archive'},{image,keyPath:'raw key contents',sourcePath:'/private/archive'},{image,keyPath:'/private/key,target=/evil',sourcePath:'/private/archive'},{image,keyPath:'/private/key',sourcePath:'/private/archive\nPRIVATE=data'}])expect(validVerificationInput(input)).toBe(false);
 });
 it('never treats restore, unknown fields, unsafe counters or incomplete receipts as authenticated success',()=>{
  expect(parseVerificationReceipt(verification).operation).toBe('verified');
  for(const value of [{...verification,operation:'restored'},{...verification,private:'key contents'},{...verification,files:0},{...verification,files:1000001},{...verification,authenticatedManifestSha256:'bad'},{...verification,id:'synthetic'},{...verification,at:Number.MAX_SAFE_INTEGER+1}])expect(()=>parseVerificationReceipt(value)).toThrow('MANAGER_PROTOCOL');
 });
});
