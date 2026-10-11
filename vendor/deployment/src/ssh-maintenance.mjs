// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
// Optional remote native maintenance actions. No independent update engine.
const fail=code=>{throw Object.assign(Error(code),{code});};
const closed=(v,keys)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===keys.sort().join(',');
const path=v=>typeof v==='string'&&v.startsWith('/')&&v.length<4096&&!/[\0\r\n]/.test(v);
const hash=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(v);
export function maintenanceOptions(action,v){
 const base=['profilePath','updatePath','updateSha256','operationId','expectedGeneration'];
 const full=['artifactPath','stagingParent','pythonPath','sourceCommit','maintenanceImage','exportParent','hostArchivePath','trustArchivePath','keyPath','pairBindingPath','hostExtractionPath'];
 if(action==='backup'){if(!closed(v,['maintenanceImage','keyPath'])||!path(v.keyPath)||!/^sha256:[a-f0-9]{64}$/.test(v.maintenanceImage))fail('SSH_BACKUP_INPUT_INVALID');}
 else if(['update','update-status'].includes(action)){
  if(!closed(v,action==='update'?[...base,...full]:base)||!['profilePath','updatePath'].every(k=>path(v[k]))||!hash(v.updateSha256)||!uuid(v.operationId)||!Number.isSafeInteger(v.expectedGeneration)||v.expectedGeneration<1)fail('SSH_UPDATE_INPUT_INVALID');
  if(action==='update'&&(!full.filter(k=>!['sourceCommit','maintenanceImage'].includes(k)).every(k=>path(v[k]))||!/^[a-f0-9]{40}$/.test(v.sourceCommit)||!/^sha256:[a-f0-9]{64}$/.test(v.maintenanceImage)))fail('SSH_UPDATE_INPUT_INVALID');
 }else if(action!=='backup-jobs'||v!==null)fail('SSH_MAINTENANCE_INPUT_INVALID');
 return structuredClone(v);
}
export function projectMaintenance(action,r,operationId){
 if(r?.code)fail(/^SSH_[A-Z_]+$/.test(r.code)?r.code:'SSH_MAINTENANCE_REFUSED');
 if(action==='backup-jobs'){
  if(!closed(r,['version','action','jobs','rawLogsIncluded'])||r.version!==1||r.action!==action||r.rawLogsIncluded!==false||!Array.isArray(r.jobs)||r.jobs.length>50)fail('SSH_MANAGER_RESPONSE_INVALID');
  for(const j of r.jobs)if(!closed(j,['id','state','stage','code','createdAt','updatedAt'])||!uuid(j.id)||!['queued','running','completed','failed','interrupted','cancelled'].includes(j.state)||typeof j.stage!=='string'||!/^[a-z0-9_-]{1,64}$/.test(j.stage)||(j.code!==null&&!/^[A-Z0-9_]{1,64}$/.test(j.code))||![j.createdAt,j.updatedAt].every(Number.isSafeInteger))fail('SSH_MANAGER_RESPONSE_INVALID');
 }else if(action==='backup'){
  if(!closed(r,['version','action','state','jobId','backupId','authenticatedManifestSha256','writersPaused','preservedVolumes'])||r.version!==1||r.action!==action||r.state!=='completed'||!uuid(r.jobId)||!uuid(r.backupId)||!hash(r.authenticatedManifestSha256)||r.writersPaused!==true||r.preservedVolumes!==true)fail('SSH_BACKUP_UNVERIFIED');
 }else if(!closed(r,['version','action','operationId','state','authorityGeneration','candidateId','healthVerified','selectionVerified','updateExecuted','preservedVolumes'])||r.version!==1||r.action!==action||r.operationId!==operationId||!Number.isSafeInteger(r.authorityGeneration)||r.authorityGeneration<1||typeof r.state!=='string'||!/^[a-z_]{1,64}$/.test(r.state)||(r.candidateId!==null&&!uuid(r.candidateId))||[r.healthVerified,r.selectionVerified,r.updateExecuted].some(v=>typeof v!=='boolean')||r.preservedVolumes!==true||(action==='update'&&(r.state!=='updated'||!r.healthVerified||!r.selectionVerified||!r.updateExecuted)))fail('SSH_UPDATE_UNVERIFIED');
 return structuredClone(r);
}
// Embedded verbatim into the existing protected remote script; all resources
// remain under the same checked unprivileged home. No raw output escapes.
export function executeRemoteMaintenance(i,{fs,execFileSync,join,read,ancestors,sha,fail,boundedJSON,uuid,hash}){
 const m=i.maintenance,action=i.action;
 const call=(executable,args)=>{try{const v=boundedJSON(execFileSync(executable,args,{timeout:action==='update'?2400000:action==='backup'?1800000:110000,maxBuffer:262144,stdio:['ignore','pipe','ignore']}).toString(),262144);if(v?.code)fail('SSH_NATIVE_REFUSED');return v;}catch{fail('SSH_MAINTENANCE_UNCERTAIN');}};
 const safe=p=>{ancestors(p);if(fs.realpathSync(p)!==p)fail('SSH_MAINTENANCE_INPUT_UNSAFE');};
 if(action==='backup-jobs'){
  const jobs=call(i.managerPath,['--root',i.remoteRoot,'backup-jobs']);if(!Array.isArray(jobs)||jobs.length>500)fail('SSH_MANAGER_RESPONSE_INVALID');
  return {version:1,action,jobs:jobs.slice(-50).map(j=>({id:j.id,state:j.state,stage:j.stage,code:/^[A-Z0-9_]{1,64}$/.test(j.errorCode??'')?j.errorCode:null,createdAt:j.createdAt,updatedAt:j.updatedAt})),rawLogsIncluded:false};
 }
 safe(m.keyPath??m.profilePath);
 if(action==='backup'){
  const key=read(m.keyPath,32);if(key.length!==32||m.keyPath.startsWith(i.remoteRoot+'/'))fail('SSH_BACKUP_INPUT_UNSAFE');
  const r=call(i.managerPath,['--root',i.remoteRoot,'create-backup',m.maintenanceImage,m.keyPath,'--external-writers-quiesced']);if(!read(m.keyPath,32).equals(key))fail('SSH_MAINTENANCE_INPUT_CHANGED');
  if(r.operation!=='created-and-authenticated'||!uuid(r.id)||!uuid(r.backupId)||!hash(r.authenticatedManifestSha256)||r.writersPaused!==true)fail('SSH_BACKUP_UNVERIFIED');
  return {version:1,action,state:'completed',jobId:r.id,backupId:r.backupId,authenticatedManifestSha256:r.authenticatedManifestSha256,writersPaused:true,preservedVolumes:true};
 }
 safe(m.profilePath);safe(m.updatePath);const root=fs.lstatSync(m.profilePath);if(!root.isDirectory()||(root.mode&511)!==448)fail('SSH_UPDATE_INPUT_UNSAFE');const executable=read(m.updatePath,33554432,true);if(sha(executable)!==m.updateSha256)fail('SSH_UPDATE_EXECUTABLE_CHANGED');
 const update=(command,args=[])=>call(m.updatePath,[command,'--profile',m.profilePath,'--installation','default',...args,'--apps-closed']);
 const before=update('update-intent'),intent=before?.intent?.update,plan=intent?.plan;
 if(!Number.isSafeInteger(before?.trust?.generation)||before.trust.generation<m.expectedGeneration||(action==='update'&&before.trust.generation!==m.expectedGeneration)||plan?.operationId!==m.operationId)fail('SSH_UPDATE_AUTHORITY_CHANGED');
 const source=plan.sourceInstance==='default'?join(m.profilePath,'local-runtime'):join(m.profilePath,'installations',plan.sourceInstance);if(source!==i.remoteRoot)fail('SSH_UPDATE_SOURCE_MISMATCH');
 let result=null;
 if(action==='update'){
  if(intent.stage!=='prepared')fail('SSH_UPDATE_STAGE_INVALID');
  for(const k of ['artifactPath','stagingParent','pythonPath','exportParent','hostArchivePath','trustArchivePath','keyPath','pairBindingPath','hostExtractionPath'])safe(m[k]);
  const args=['--artifact',m.artifactPath,'--staging-parent',m.stagingParent,'--python',m.pythonPath,'--source-commit',m.sourceCommit,'--maintenance-image',m.maintenanceImage,'--export-parent',m.exportParent,'--host-archive',m.hostArchivePath,'--trust-archive',m.trustArchivePath,'--key',m.keyPath,'--pair-binding',m.pairBindingPath,'--host-extraction',m.hostExtractionPath,'--operation-id',m.operationId,'--expected-generation',String(m.expectedGeneration),'--external-writers-quiesced'];
  result=update('execute-full-update',args);
 }
 if(sha(read(m.updatePath,33554432,true))!==m.updateSha256)fail('SSH_UPDATE_EXECUTABLE_CHANGED');
 const cold=update('update-intent'),current=cold?.intent?.update;
 if(current?.plan?.operationId!==m.operationId||JSON.stringify(current.plan)!==JSON.stringify(plan)||!Number.isSafeInteger(cold?.trust?.generation)||(action==='update-status'&&cold.trust.generation<m.expectedGeneration))fail('SSH_UPDATE_UNVERIFIED');
 const receipt=result?.selection,candidate=current?.health?.instanceId??null;
 let health=false,selected=false;
 if(action==='update'){
  if(result?.updateExecuted!==true||receipt?.selectionCompleted!==true||receipt.operationId!==m.operationId||!uuid(candidate)||candidate!==plan.targetInstance||receipt.runtimeReplayed!==false||receipt.healthReplayed!==false||!hash(receipt.authorityHeadSha256)||current.stage!=='updated'||cold.trust.generation!==receipt.authorityGeneration||current.health?.ready!==true||current.health.instanceId!==candidate||current.health.image!==plan.targetImage||current.health.schema!==plan.targetSchema)fail('SSH_UPDATE_UNVERIFIED');
  const selection=boundedJSON(read(join(m.profilePath,'installation-selection.json'),4194304).toString(),4194304);selected=selection.activeId===candidate&&selection.installations?.some(v=>v.id===candidate)===true;
  const candidateRoot=join(m.profilePath,'installations',candidate);safe(candidateRoot);const status=call(i.managerPath,['--root',candidateRoot,'status']);health=status.installed===true&&status.readiness?.ready===true;
  if(!health||!selected)fail('SSH_UPDATE_UNVERIFIED');
 }
 return {version:1,action,operationId:m.operationId,state:current.stage,authorityGeneration:cold.trust.generation,candidateId:candidate,healthVerified:health,selectionVerified:selected,updateExecuted:action==='update',preservedVolumes:true};
}
