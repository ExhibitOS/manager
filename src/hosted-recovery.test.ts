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

it('keeps companion delivery optional, archive bound and distinct from encrypted archive contents',()=>{
 const archiveSaved={jobId:id,backupId:id,path:'/synthetic/ExhibitOS-backup-copy',files:4,bytes:400};
 const base={source,jobs:[job],keySaved:false,archiveSaved};
 const copy={jobId:id,state:'copied',path:'/synthetic/ExhibitOS-publication',files:3,bytes:100};
 expect(parseHostedRecovery({...base,publicationSaved:copy}).publicationSaved).toEqual(copy);
 expect(parseHostedRecovery({...base,publicationSaved:{...copy,state:'unavailable',path:null,files:0,bytes:0}}).archiveSaved).toEqual(archiveSaved);
 expect(parseHostedRecovery(base).publicationSaved).toBeNull();
 for(const publicationSaved of [{...copy,path:'relative'},{...copy,path:'/bad\npath'},{...copy,token:'forbidden'},{...copy,state:'unavailable'},{...copy,jobId:'00000000-0000-4000-8000-000000000002'}])expect(()=>parseHostedRecovery({...base,publicationSaved})).toThrow('MANAGER_PROTOCOL');
 expect(()=>parseHostedRecovery({...base,archiveSaved:null,publicationSaved:copy})).toThrow('MANAGER_PROTOCOL');
});

const remoteTarget={sourceJobId:id,workspaceId:source.workspaceId,postgresId:'dpg-restored-copy',name:'Synthetic separate recovery DB'};
const remoteOperation={id:'00000000-0000-4000-8000-000000000003',sourceJobId:id,source,target:{workspaceId:source.workspaceId,postgresId:remoteTarget.postgresId,createRequested:false},expectedBackupId:job.receipt.backupId,expectedManifestSha256:job.receipt.authenticatedManifestSha256,expectedFiles:job.receipt.files,sourceJobSha256:'c'.repeat(64),targetIdentitySha256:'d'.repeat(64),state:'verified',errorCode:null};
const remoteContext={source,jobs:[job],restoreChecks:[check],remoteTargets:[remoteTarget],remoteRestores:[remoteOperation],keySaved:false,archiveSaved:null};

describe('remote recovery native response boundaries',()=>{
 it('preserves distinct archive, local-copy, remote-copy and service-connection results',()=>{
  const remoteCopy=parseHostedRecovery(remoteContext);
  expect(remoteCopy.jobs[0]?.state).toBe('completed');
  expect(remoteCopy.restoreChecks[0]?.receipt?.operation).toBe('restored-and-verified');
  expect(remoteCopy.remoteRestores[0]?.state).toBe('verified');
  expect(remoteCopy.remoteTargets).toEqual([remoteTarget]);
  expect(remoteCopy.keySaved).toBe(false);
  for(const state of ['connecting','connected','connection-restored','connection-recovery-required']){
   const result=parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,state,errorCode:state==='connection-restored'||state==='connection-recovery-required'?'HOSTED_REMOTE_CONNECTION_RECOVERY_REQUIRED':null}]});
   expect(result.remoteRestores[0]?.state).toBe(state);
   expect(result.jobs[0]?.receipt).toEqual(job.receipt);
   expect(result.restoreChecks[0]?.state).toBe('completed');
  }
  const legacy=parseHostedRecovery({source,jobs:[job],keySaved:false,archiveSaved:null});
  expect(legacy.remoteTargets).toEqual([]);expect(legacy.remoteRestores).toEqual([]);
 });

 it('retains an unfinished new-target request without accepting it as a verified DB or connection',()=>{
  const waiting={...remoteOperation,target:{workspaceId:source.workspaceId,postgresId:null,createRequested:true},targetIdentitySha256:null,state:'prepared'};
  expect(parseHostedRecovery({...remoteContext,remoteRestores:[waiting]}).remoteRestores[0]).toEqual(waiting);
  for(const state of ['verified','connecting','connected','connection-restored']){
   expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...waiting,state}]})).toThrow('MANAGER_PROTOCOL');
   expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,state,targetIdentitySha256:null}]})).toThrow('MANAGER_PROTOCOL');
  }
 });

 it('rejects another backup or native job being substituted for the original archive',()=>{
  const anotherSource={...source,serviceId:'srv-another',postgresId:'dpg-another'};
  const anotherJob={...job,id:'00000000-0000-4000-8000-000000000004',source:anotherSource,receipt:{...job.receipt,...anotherSource,backupId:'00000000-0000-4000-8000-000000000005'}};
  for(const patch of [
   {sourceJobId:anotherJob.id},
   {source:{...source,workspaceId:'tea-another'}},
   {source:{...source,serviceId:anotherSource.serviceId}},
   {source:{...source,postgresId:anotherSource.postgresId}},
   {expectedBackupId:anotherJob.receipt.backupId},
   {expectedManifestSha256:'e'.repeat(64)},
   {expectedFiles:job.receipt.files+1},
  ])expect(()=>parseHostedRecovery({...remoteContext,jobs:[job,anotherJob],remoteRestores:[{...remoteOperation,...patch}]})).toThrow('MANAGER_PROTOCOL');
 });

 it('refuses original-DB targets, foreign workspaces and a duplicated operation retargeted within one response',()=>{
  for(const target of [
   {...remoteOperation.target,postgresId:source.postgresId},
   {...remoteOperation.target,workspaceId:'tea-another'},
  ])expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,target}]})).toThrow('MANAGER_PROTOCOL');
  const retargeted={...remoteOperation,target:{...remoteOperation.target,postgresId:'dpg-second-copy'}};
  expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[remoteOperation,retargeted]})).toThrow('MANAGER_PROTOCOL');
 });

 it('shows only candidate selectors tied to an existing completed backup in its original workspace',()=>{
  for(const patch of [
   {sourceJobId:'00000000-0000-4000-8000-000000000099'},
   {workspaceId:'tea-another'},
   {postgresId:source.postgresId},
  ])expect(()=>parseHostedRecovery({...remoteContext,remoteTargets:[{...remoteTarget,...patch}]})).toThrow('MANAGER_PROTOCOL');
  const failedOriginal={...job,state:'failed',stage:'failed',errorCode:'HOSTED_RECOVERY_EXECUTION_FAILED',receipt:null};
  expect(()=>parseHostedRecovery({...remoteContext,jobs:[failedOriginal],restoreChecks:[],remoteRestores:[]})).toThrow('MANAGER_PROTOCOL');
 });

 it('rejects secret-bearing native additions instead of letting selectors or results carry private inputs',()=>{
  for(const extra of [
   {token:'synthetic-not-a-real-provider-token'},
   {keyBase64:'synthetic-not-a-backup-key'},
   {databaseURL:'postgresql://synthetic.invalid/db'},
   {plan:{originalEnvironment:[{key:'ADMIN_PASSWORD',value:'synthetic-secret'}]}},
   {creationProof:{postgresId:remoteTarget.postgresId}},
  ])expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,...extra}]})).toThrow('MANAGER_PROTOCOL');
  expect(()=>parseHostedRecovery({...remoteContext,remoteTargets:[{...remoteTarget,externalConnectionString:'postgresql://synthetic.invalid/db'}]})).toThrow('MANAGER_PROTOCOL');
  expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,target:{...remoteOperation.target,password:'synthetic-secret'}}]})).toThrow('MANAGER_PROTOCOL');
  expect(()=>parseHostedRecovery({...remoteContext,remoteRestores:[{...remoteOperation,source:{...source,token:'synthetic-secret'}}]})).toThrow('MANAGER_PROTOCOL');
 });
});
