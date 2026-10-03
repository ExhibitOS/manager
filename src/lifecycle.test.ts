// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseInstallationContext,parseJob,parseStatus,managerError,formatBytes,statusLabel,validVerificationInput,parseVerificationReceipt,validCreationInput,parseCreationReceipt,parseBackupJob,validRestorationInput,parseRestorationContext,parseRestorationReceipt} from './lifecycle';
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

const creation={id:verification.id,backupId:'23456789-1234-1234-1234-123456789012',operation:'created-and-authenticated',files:11,image,authenticatedManifestSha256:'b'.repeat(64),writersPaused:true,at:2};
const backupJob={id:verification.id,operation:'create',state:'completed',stage:'complete',errorCode:null,createdAt:1,updatedAt:2};
describe('Backup creation receipt and journal boundary',()=>{
 it('requires distinct writer and downtime acknowledgements plus safe paths',()=>{
  const input={image,keyPath:'/private/key',externalWritersQuiesced:true,downtimeAccepted:true};
  expect(validCreationInput(input)).toBe(true);
  for(const value of [{...input,externalWritersQuiesced:false},{...input,downtimeAccepted:false},{...input,keyPath:'private key contents'},{...input,keyPath:'/private/key,target=/host'},{...input,image:'trusted:latest'}])expect(validCreationInput(value)).toBe(false);
 });
 it('refuses unverified copies, unknown fields and unbound writer claims',()=>{
  expect(parseCreationReceipt(creation).writersPaused).toBe(true);
  for(const value of [{...creation,operation:'created'},{...creation,operation:'restored'},{...creation,writersPaused:false},{...creation,backupId:'unsafe/path'},{...creation,files:0},{...creation,private:'secret'},{...creation,authenticatedManifestSha256:'bad'}])expect(()=>parseCreationReceipt(value)).toThrow('MANAGER_PROTOCOL');
 });
 it('keeps interrupted jobs separate from completed authenticated receipts',()=>{
  expect(parseBackupJob(backupJob).state).toBe('completed');
  expect(parseBackupJob({...backupJob,state:'interrupted',stage:'pausing-writers',errorCode:'INTERRUPTED'}).state).toBe('interrupted');
  for(const value of [{...backupJob,stage:'saving-images'},{...backupJob,errorCode:'PRIVATE'},{...backupJob,state:'interrupted',errorCode:null},{...backupJob,state:'running',errorCode:'INTERRUPTED'},{...backupJob,updatedAt:0},{...backupJob,updatedAt:Number.MAX_SAFE_INTEGER},{...backupJob,private:'secret'}])expect(()=>parseBackupJob(value)).toThrow('MANAGER_PROTOCOL');
 });
});

const restored={id:verification.id,operation:'restored-and-running',backupId:creation.backupId,authenticatedManifestSha256:'b'.repeat(64),bundleId:'34567890-1234-1234-1234-123456789012',projectName:'exhibitos-34567890-1234-1234-1234-123456789012',openUrl:'http://127.0.0.1:4500',at:2};
const restoredJob={id:verification.id,state:'completed',stage:'complete',errorCode:null,createdAt:1,updatedAt:2};
describe('Fresh restoration native boundary',()=>{
 it('requires fresh acknowledgement and rejects unsafe ports and mount paths',()=>{
  const input={image,keyPath:'/private/key',sourcePath:'/private/archive',port:4500,freshInstallationAccepted:true};
  expect(validRestorationInput(input)).toBe(true);
  for(const value of [{...input,freshInstallationAccepted:false},{...input,port:80},{...input,port:65536},{...input,port:4500.5},{...input,port:NaN},{...input,keyPath:'key bytes'},{...input,sourcePath:'/private/source,target=/host'}])expect(validRestorationInput(value)).toBe(false);
 });
 it('does not accept verification-only receipts or external exhibition addresses',()=>{
  expect(parseRestorationReceipt(restored).openUrl).toBe('http://127.0.0.1:4500');
  for(const value of [{...restored,operation:'verified'},{...restored,openUrl:'https://example.com'},{...restored,openUrl:'http://127.0.0.1:4500/?secret=x'},{...restored,openUrl:'http://127.0.0.1:80'},{...restored,projectName:'foreign'},{...restored,private:'secret'}])expect(()=>parseRestorationReceipt(value)).toThrow('MANAGER_PROTOCOL');
 });
 it('requires completed history to bind an actual receipt and fences incomplete candidates',()=>{
  expect(parseRestorationContext({fresh:true,job:null,receipt:null}).fresh).toBe(true);
  expect(parseRestorationContext({fresh:false,job:restoredJob,receipt:restored}).receipt?.backupId).toBe(creation.backupId);
  expect(parseRestorationContext({fresh:false,job:{...restoredJob,state:'interrupted',stage:'checking-runtime',errorCode:'INTERRUPTED'},receipt:null}).job?.state).toBe('interrupted');
  for(const value of [{fresh:true,job:restoredJob,receipt:restored},{fresh:false,job:restoredJob,receipt:null},{fresh:false,job:restoredJob,receipt:{...restored,id:creation.backupId}},{fresh:false,job:{...restoredJob,updatedAt:0},receipt:restored},{fresh:false,job:{...restoredJob,state:'failed'},receipt:null},{fresh:false,job:{...restoredJob,stage:'unknown'},receipt:restored},{fresh:false,job:null,receipt:restored},{fresh:false,job:null,receipt:null,private:'secret'}])expect(()=>parseRestorationContext(value)).toThrow('MANAGER_PROTOCOL');
 });
});

const rootId='56789012-1234-1234-1234-123456789012';
const selection={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:rootId,mode:'managed',installations:[{id:rootId,kind:'default',createdAt:1,path:'/private/tmp/profile/local-runtime',available:true}],errorCode:null};
describe('Installation registry boundary',()=>{
 it('preserves unavailable roots for recovery without claiming availability',()=>{expect(parseInstallationContext(selection).activeId).toBe(rootId);expect(parseInstallationContext({...selection,installations:[{...selection.installations[0],available:false}],errorCode:'INSTALLATION_ROOT_UNAVAILABLE'}).installations[0]?.available).toBe(false);});
 it('rejects duplicates, injected paths, unregistered selection and malformed tokens',()=>{
  for(const value of [{...selection,selectionToken:'bad'},{...selection,activeId:selection.selectionToken},{...selection,command:'shell'},{...selection,installations:[...selection.installations,...selection.installations]},{...selection,installations:[{...selection.installations[0],path:'relative'}]},{...selection,installations:[...selection.installations,{id:selection.selectionToken,kind:'recovery',createdAt:1,path:'/private/other',available:true}]}])expect(()=>parseInstallationContext(value)).toThrow('MANAGER_PROTOCOL');
 });
});
