// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
export type HostedBackupSource={workspaceId:string;serviceId:string;postgresId:string};
export type HostedBackupJob={id:string;source:HostedBackupSource;image:string;jobBudgetBytes:number|null;state:'running'|'completed'|'failed'|'interrupted';stage:string;errorCode:string|null;receipt:({backupId:string;files:number;authenticatedManifestSha256:string}&HostedBackupSource)|null;createdAt:number;updatedAt:number};
export type HostedRestoreCheck={id:string;sourceJobId:string;source:HostedBackupSource;image:string;expectedBackupId:string;expectedManifestSha256:string;expectedFiles:number;jobBudgetBytes:number;state:'running'|'completed'|'failed'|'interrupted';stage:string;errorCode:string|null;receipt:{operation:'restored-and-verified';backupId:string;authenticatedManifestSha256:string;files:number}|null;createdAt:number;updatedAt:number};
export type HostedPublicationCopy={jobId:string;state:'copied'|'unavailable';files:number;bytes:number};
export type HostedPublicationSaved=HostedPublicationCopy&{path:string|null};
export type HostedRemoteTarget={sourceJobId:string;workspaceId:string;postgresId:string;name:string};
export type HostedRemoteRestore={id:string;sourceJobId:string;source:HostedBackupSource;target:{workspaceId:string;postgresId:string|null;createRequested:boolean};expectedBackupId:string;expectedManifestSha256:string;expectedFiles:number;sourceJobSha256:string;targetIdentitySha256:string|null;state:'prepared'|'dispatched'|'verified'|'recovery-required'|'connecting'|'connected'|'connection-restored'|'connection-recovery-required';errorCode:string|null};
export type HostedRecoveryContext={remoteTargets:HostedRemoteTarget[];remoteRestores:HostedRemoteRestore[];publicationSaved:HostedPublicationSaved|null;publicationCopies:HostedPublicationCopy[];source:HostedBackupSource|null;jobs:HostedBackupJob[];restoreChecks:HostedRestoreCheck[];keySaved:boolean;archiveSaved:{jobId:string;backupId:string;path:string;files:number;bytes:number}|null};
export type HostedRecoveryInput={action:'context'|'jobs'|'create'|'export-key'|'export-archive'|'check-restore'|'remote-targets'|'remote-restore'|'remote-connect';sourceCidr?:string;confirmExternalWritersStopped?:boolean;jobId?:string;targetPostgresId?:string;operationId?:string;createNewTarget?:boolean;confirmServiceConnection?:boolean;expectedSource?:HostedBackupSource;jobBudgetBytes?:number};
// Blank keeps native automatic budgeting; null is an invalid user limit.
export function hostedBackupBudgetBytes(value:string):number|undefined|null{
 const text=value.trim();if(!text)return undefined;
 if(!/^\d+$/.test(text))return null;
 const gib=Number(text),bytes=gib*1024**3;
 return Number.isSafeInteger(gib)&&gib>=1&&Number.isSafeInteger(bytes)?bytes:null;
}
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
const keys=(v:Record<string,unknown>,fields:string[])=>Object.keys(v).sort().join(',')===[...fields].sort().join(',');
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
const digest=(v:unknown)=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const number=(v:unknown):v is number=>Number.isSafeInteger(v)&&Number(v)>=0;
function source(v:unknown):v is HostedBackupSource{return object(v)&&keys(v,['workspaceId','serviceId','postgresId'])&&Object.values(v).every(s=>typeof s==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(s));}
export function parseHostedRecovery(v:unknown):HostedRecoveryContext{
 if(!object(v)||!keys(v,['source','jobs','keySaved','archiveSaved',...(v.restoreChecks!==undefined?['restoreChecks']:[]),...(v.publicationCopies!==undefined?['publicationCopies']:[]),...(v.publicationSaved!==undefined?['publicationSaved']:[]),...(v.remoteTargets!==undefined?['remoteTargets']:[]),...(v.remoteRestores!==undefined?['remoteRestores']:[])])||!(v.source===null||source(v.source))||typeof v.keySaved!=='boolean'||!Array.isArray(v.jobs)||v.jobs.length>10000)throw Error('MANAGER_PROTOCOL');
 for(const job of v.jobs){
  if(!object(job)||!keys(job,['id','source','image','jobBudgetBytes','state','stage','errorCode','receipt','createdAt','updatedAt'])||!(job.jobBudgetBytes===null||number(job.jobBudgetBytes)&&job.jobBudgetBytes>=4194304)||!uuid(job.id)||!source(job.source)||typeof job.image!=='string'||!/^sha256:[a-f0-9]{64}$/.test(job.image)||!['running','completed','failed','interrupted'].includes(String(job.state))||!['checking-source-and-creating','completed','failed','interrupted'].includes(String(job.stage))||!number(job.createdAt)||!number(job.updatedAt)||job.updatedAt<job.createdAt||job.updatedAt>8640000000000000||!(job.errorCode===null||typeof job.errorCode==='string'&&/^[A-Z_]{1,128}$/.test(job.errorCode))||(['failed','interrupted'].includes(String(job.state))!==(job.errorCode!==null)))throw Error('MANAGER_PROTOCOL');
  if(job.state==='completed'){
   const r=job.receipt;
   if(!object(r)||!keys(r,['provider','action','workspaceId','serviceId','postgresId','operation','backupId','files','authenticatedManifestSha256',...(r.expiresAt!==undefined?['expiresAt']:[])])||r.provider!=='render'||r.action!=='local-encrypted-backup'||r.operation!=='created-and-verified'||!uuid(r.backupId)||!number(r.files)||r.files<1||r.files>100000||!digest(r.authenticatedManifestSha256)||r.workspaceId!==job.source.workspaceId||r.serviceId!==job.source.serviceId||r.postgresId!==job.source.postgresId||(r.expiresAt!==undefined&&(typeof r.expiresAt!=='string'||r.expiresAt.length>32||!Number.isFinite(Date.parse(r.expiresAt)))))throw Error('MANAGER_PROTOCOL');
  }else if(job.receipt!==null)throw Error('MANAGER_PROTOCOL');
 }
 const restoreChecks=v.restoreChecks??[];
 if(!Array.isArray(restoreChecks)||restoreChecks.length>10000)throw Error('MANAGER_PROTOCOL');
 for(const check of restoreChecks){
  if(!object(check)||!keys(check,['id','sourceJobId','source','image','expectedBackupId','expectedManifestSha256','expectedFiles','jobBudgetBytes','state','stage','errorCode','receipt','createdAt','updatedAt'])||!uuid(check.id)||!uuid(check.sourceJobId)||check.id===check.sourceJobId||!source(check.source)||typeof check.image!=='string'||!/^sha256:[a-f0-9]{64}$/.test(check.image)||!uuid(check.expectedBackupId)||!digest(check.expectedManifestSha256)||!number(check.expectedFiles)||check.expectedFiles<1||check.expectedFiles>100000||!number(check.jobBudgetBytes)||check.jobBudgetBytes<4194304||check.jobBudgetBytes>32*1024**3||!['running','completed','failed','interrupted'].includes(String(check.state))||check.stage!==(check.state==='running'?'checking-archive-and-restoring':check.state)||!number(check.createdAt)||!number(check.updatedAt)||check.createdAt>check.updatedAt||check.updatedAt>8640000000000000||!(check.errorCode===null||typeof check.errorCode==='string'&&/^[A-Z_]{1,128}$/.test(check.errorCode))||(['failed','interrupted'].includes(String(check.state))!==(check.errorCode!==null)))throw Error('MANAGER_PROTOCOL');
  const checkSource=check.source;
  const original=v.jobs.find(job=>job.id===check.sourceJobId);
  if(!original||original.state!=='completed'||original.receipt?.backupId!==check.expectedBackupId||original.receipt?.authenticatedManifestSha256!==check.expectedManifestSha256||original.receipt?.files!==check.expectedFiles||(['workspaceId','serviceId','postgresId'] as const).some(key=>original.source[key]!==checkSource[key]))throw Error('MANAGER_PROTOCOL');
  if(check.state==='completed'){
   const r=check.receipt;if(!object(r)||!keys(r,['operation','backupId','authenticatedManifestSha256','files'])||r.operation!=='restored-and-verified'||r.backupId!==check.expectedBackupId||r.authenticatedManifestSha256!==check.expectedManifestSha256||r.files!==check.expectedFiles)throw Error('MANAGER_PROTOCOL');
  }else if(check.receipt!==null)throw Error('MANAGER_PROTOCOL');
 }
 if(v.archiveSaved!==null){const r=v.archiveSaved;if(!object(r)||!keys(r,['jobId','backupId','path','files','bytes'])||!uuid(r.jobId)||!uuid(r.backupId)||typeof r.path!=='string'||!r.path.startsWith('/')||/[\x00-\x1f]/.test(r.path)||!number(r.files)||r.files<4||r.files>100003||!number(r.bytes)||r.bytes===0)throw Error('MANAGER_PROTOCOL');}
 const publicationCopies=v.publicationCopies??[];
 if(!Array.isArray(publicationCopies)||publicationCopies.length>v.jobs.length)throw Error('MANAGER_PROTOCOL');
 const seenCopies=new Set<string>();
 for(const copy of publicationCopies){
  if(!object(copy)||!keys(copy,['jobId','state','files','bytes'])||!uuid(copy.jobId)||seenCopies.has(copy.jobId)||!v.jobs.some(job=>job.id===copy.jobId)||!['copied','unavailable'].includes(String(copy.state))||!number(copy.files)||!number(copy.bytes)||(copy.state==='copied'?(copy.files<3||copy.files>20000||copy.bytes===0):(copy.files!==0||copy.bytes!==0)))throw Error('MANAGER_PROTOCOL');
  const original=v.jobs.find(job=>job.id===copy.jobId);
  if(copy.state==='copied'&&(original?.state!=='completed'||!number(original.jobBudgetBytes)||copy.bytes>original.jobBudgetBytes))throw Error('MANAGER_PROTOCOL');
  seenCopies.add(copy.jobId);
 }
 const publicationSaved=v.publicationSaved??null;
 if(publicationSaved!==null){
  const copy=publicationSaved;
  if(!object(copy)||!keys(copy,['jobId','state','path','files','bytes'])||!uuid(copy.jobId)||!object(v.archiveSaved)||copy.jobId!==v.archiveSaved.jobId||!['copied','unavailable'].includes(String(copy.state))||!(copy.path===null||typeof copy.path==='string'&&copy.path.startsWith('/')&&copy.path.length<=2048&&!/[\x00-\x1f]/.test(copy.path))||!number(copy.files)||!number(copy.bytes)||(copy.state==='copied'?(copy.path===null||copy.files<3||copy.files>20000||copy.bytes===0):(copy.files!==0||copy.bytes!==0)))throw Error('MANAGER_PROTOCOL');
  const original=v.jobs.find(job=>job.id===copy.jobId);
  if(!original||original.state!=='completed'||copy.state==='copied'&&(!number(original.jobBudgetBytes)||copy.bytes>original.jobBudgetBytes))throw Error('MANAGER_PROTOCOL');
 }
 const remoteTargets=v.remoteTargets??[],remoteRestores=v.remoteRestores??[];
 if(!Array.isArray(remoteTargets)||remoteTargets.length>10000||!Array.isArray(remoteRestores)||remoteRestores.length>10000)throw Error('MANAGER_PROTOCOL');
 for(const target of remoteTargets){
  if(!object(target)||!keys(target,['sourceJobId','workspaceId','postgresId','name'])||!uuid(target.sourceJobId)||typeof target.name!=='string'||target.name.length>128||/[\x00-\x1f\x7f]/.test(target.name)||![target.workspaceId,target.postgresId].every(s=>typeof s==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(s)))throw Error('MANAGER_PROTOCOL');
  const original=v.jobs.find(j=>j.id===target.sourceJobId);if(original?.state!=='completed'||original.source.workspaceId!==target.workspaceId||original.source.postgresId===target.postgresId)throw Error('MANAGER_PROTOCOL');
 }
 const remoteIds=new Set();
 for(const op of remoteRestores){
  if(!object(op)||!keys(op,['id','sourceJobId','source','target','expectedBackupId','expectedManifestSha256','expectedFiles','sourceJobSha256','targetIdentitySha256','state','errorCode'])||!uuid(op.id)||remoteIds.has(op.id)||!uuid(op.sourceJobId)||!source(op.source)||!object(op.target)||!keys(op.target,['workspaceId','postgresId','createRequested'])||!(typeof op.target.createRequested==='boolean'&&typeof op.target.workspaceId==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(op.target.workspaceId)&&(op.target.postgresId===null?op.target.createRequested:typeof op.target.postgresId==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(op.target.postgresId)))||op.target.workspaceId!==op.source.workspaceId||op.target.postgresId===op.source.postgresId||!digest(op.sourceJobSha256)||!(op.targetIdentitySha256===null||digest(op.targetIdentitySha256))||!['prepared','dispatched','verified','recovery-required','connecting','connected','connection-restored','connection-recovery-required'].includes(String(op.state))||!(op.errorCode===null||typeof op.errorCode==='string'&&/^[A-Z_]{1,128}$/.test(op.errorCode)))throw Error('MANAGER_PROTOCOL');
  if(['verified','connecting','connected','connection-restored','connection-recovery-required'].includes(String(op.state))&&(!digest(op.targetIdentitySha256)||op.target.postgresId===null))throw Error('MANAGER_PROTOCOL');
  const opSource=op.source;const original=v.jobs.find(j=>j.id===op.sourceJobId);if(original?.state!=='completed'||original.receipt?.backupId!==op.expectedBackupId||original.receipt?.authenticatedManifestSha256!==op.expectedManifestSha256||original.receipt?.files!==op.expectedFiles||(['workspaceId','serviceId','postgresId'] as const).some(k=>original.source[k]!==opSource[k]))throw Error('MANAGER_PROTOCOL');remoteIds.add(op.id);
 }
 return {...v,restoreChecks,publicationCopies,publicationSaved,remoteTargets,remoteRestores} as unknown as HostedRecoveryContext;
}
export const hostedRecoveryClient={native:isTauri,async operation(selectionToken:string,input:HostedRecoveryInput){if(!['context','jobs','create','export-key','export-archive','check-restore','remote-targets','remote-restore','remote-connect'].includes(input.action))throw Error('MANAGER_PROTOCOL');return parseHostedRecovery(await invoke('manager_hosted_recovery',{selectionToken,input}));}};
