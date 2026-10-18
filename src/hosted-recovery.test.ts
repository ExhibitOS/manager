// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseHostedRecovery} from './hosted-recovery';
const id='00000000-0000-4000-8000-000000000001';
const source={workspaceId:'tea-fixture',serviceId:'srv-fixture',postgresId:'dpg-fixture'};
const job={id,source,image:`sha256:${'a'.repeat(64)}`,jobBudgetBytes:64*1024**3,state:'completed',stage:'completed',errorCode:null,createdAt:1,updatedAt:2,receipt:{provider:'render',action:'local-encrypted-backup',operation:'created-and-verified',...source,backupId:id,files:1,authenticatedManifestSha256:'b'.repeat(64)}};
describe('hosted backup native results',()=>{
 it('keeps source budget and completed archive delivery separate from key delivery',()=>{
  const archiveSaved={jobId:id,backupId:id,path:'/synthetic/ExhibitOS-backup-copy',files:4,bytes:400};
  const result=parseHostedRecovery({source,jobs:[job],keySaved:false,archiveSaved});
  expect(result.archiveSaved).toEqual(archiveSaved);expect(result.keySaved).toBe(false);
  expect(parseHostedRecovery({source:null,jobs:[{...job,jobBudgetBytes:null}],keySaved:false,archiveSaved:null}).jobs).toHaveLength(1);
 });
 it('rejects mismatched source, private extra fields and malformed archive receipts',()=>{
  for(const patch of [{receipt:{...job.receipt,postgresId:'dpg-other'}},{jobBudgetBytes:1},{secret:'must-not-cross'}])expect(()=>parseHostedRecovery({source,jobs:[{...job,...patch}],keySaved:false,archiveSaved:null})).toThrow('MANAGER_PROTOCOL');
  for(const archiveSaved of [{jobId:id,backupId:id,path:'relative',files:4,bytes:2},{jobId:id,backupId:id,path:'/safe',files:4,bytes:0}])expect(()=>parseHostedRecovery({source,jobs:[job],keySaved:false,archiveSaved})).toThrow('MANAGER_PROTOCOL');
 });
});

const check={id:'00000000-0000-4000-8000-000000000002',sourceJobId:id,source,image:job.image,expectedBackupId:id,expectedManifestSha256:job.receipt.authenticatedManifestSha256,expectedFiles:1,jobBudgetBytes:1024**3,state:'completed',stage:'completed',errorCode:null,receipt:{operation:'restored-and-verified',backupId:id,authenticatedManifestSha256:job.receipt.authenticatedManifestSha256,files:1},createdAt:2,updatedAt:3};
it('isolated restore checks remain bound to their completed original and cannot report activation',()=>{
 const value={source,jobs:[job],restoreChecks:[check],keySaved:false,archiveSaved:null};expect(parseHostedRecovery(value).restoreChecks).toEqual([check]);
 expect(parseHostedRecovery({source,jobs:[job],keySaved:false,archiveSaved:null}).restoreChecks).toEqual([]);
 for(const patch of [{id},{sourceJobId:check.id},{expectedManifestSha256:'e'.repeat(64)},{source:{...source,serviceId:'srv-other'}},{jobBudgetBytes:33*1024**3},{receipt:{...check.receipt,operation:'restored-and-running'}},{activated:true},{state:'running',stage:'checking-archive-and-restoring'}])expect(()=>parseHostedRecovery({...value,restoreChecks:[{...check,...patch}]})).toThrow('MANAGER_PROTOCOL');
});

 it('keeps publication custody separate and closed without paths or secret-bearing metadata',()=>{
  const base={source,jobs:[job],keySaved:false,archiveSaved:null};
  const copy={jobId:id,state:'copied',files:3,bytes:100};
  expect(parseHostedRecovery({...base,publicationCopies:[copy]}).publicationCopies).toEqual([copy]);
  expect(parseHostedRecovery(base).publicationCopies).toEqual([]);
  expect(()=>parseHostedRecovery({...base,publicationCopies:[{...copy,bytes:job.jobBudgetBytes+1}]})).toThrow('MANAGER_PROTOCOL');
  expect(()=>parseHostedRecovery({...base,jobs:[{...job,jobBudgetBytes:null}],publicationCopies:[copy]})).toThrow('MANAGER_PROTOCOL');
  expect(parseHostedRecovery({...base,publicationCopies:[{...copy,state:'unavailable',files:0,bytes:0}]}).publicationCopies[0]?.state).toBe('unavailable');
  for(const publicationCopies of [[{...copy,sourcePath:'/private/original'}],[{...copy,tokens:'not-allowed'}],[{...copy,state:'unavailable'}],[copy,copy],[{...copy,jobId:'00000000-0000-4000-8000-000000000002'}]])expect(()=>parseHostedRecovery({...base,publicationCopies})).toThrow('MANAGER_PROTOCOL');
 });
