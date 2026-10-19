// SPDX-License-Identifier: Apache-2.0
// External archive checks remain independent of hosted completed-job authority.
export interface ExternalSource{verificationId:string;backupId:string;image:string;files:number;at:number}
export interface ExternalReceipt{operation:'restored-and-verified';backupId:string;authenticatedManifestSha256:string;files:number}
export interface ExternalCheck{id:string;verificationId:string;image:string;expectedBackupId:string;expectedManifestSha256:string;expectedFiles:number;jobBudgetBytes:number;state:'running'|'completed'|'failed'|'interrupted';stage:string;errorCode:string|null;receipt:ExternalReceipt|null;createdAt:number;updatedAt:number}
export interface ExternalContext{sources:ExternalSource[];checks:ExternalCheck[]}
export type ExternalInput={action:'context'}|{action:'check';verificationId:string;keyPath:string}|{action:'reconcile';jobId:string;preserveCandidates:true};
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
const closed=(v:unknown,keys:string):v is Record<string,unknown>=>object(v)&&Object.keys(v).sort().join(',')===keys.split(',').sort().join(',');
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(v)&&v!=='00000000-0000-0000-0000-000000000000';
const image=(v:unknown)=>typeof v==='string'&&/^sha256:[a-f0-9]{64}$/.test(v);
const digest=(v:unknown)=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const number=(v:unknown):v is number=>typeof v==='number'&&Number.isSafeInteger(v)&&v>=0;
const count=(v:unknown)=>number(v)&&v>=1&&v<=100000;
const time=(v:unknown)=>number(v)&&v<=8640000000000000;
export function validExternalInput(v:unknown):boolean{
 if(closed(v,'action')&&v.action==='context')return true;
 if(closed(v,'action,jobId,preserveCandidates'))return v.action==='reconcile'&&uuid(v.jobId)&&v.preserveCandidates===true;
 return closed(v,'action,verificationId,keyPath')&&v.action==='check'&&uuid(v.verificationId)&&typeof v.keyPath==='string'&&v.keyPath.length>0&&v.keyPath.length<=2048&&v.keyPath.startsWith('/')&&![...v.keyPath].some(c=>c.charCodeAt(0)<32||c.charCodeAt(0)===127||c===',');
}
export function parseExternalContext(value:unknown):ExternalContext{
 const fail=()=>{throw Error('MANAGER_PROTOCOL');};
 if(!closed(value,'sources,checks')||!Array.isArray(value.sources)||value.sources.length>1000||!Array.isArray(value.checks)||value.checks.length>1000)return fail();
 const sources=new Map<string,ExternalSource>(),jobs=new Set<string>();
 for(const v of value.sources){if(!closed(v,'verificationId,backupId,image,files,at')||!uuid(v.verificationId)||sources.has(v.verificationId)||!uuid(v.backupId)||!image(v.image)||!count(v.files)||!time(v.at))return fail();sources.set(v.verificationId,v as unknown as ExternalSource);}
 for(const v of value.checks){
  if(!closed(v,'id,verificationId,image,expectedBackupId,expectedManifestSha256,expectedFiles,jobBudgetBytes,state,stage,errorCode,receipt,createdAt,updatedAt')||!uuid(v.id)||jobs.has(v.id)||!uuid(v.verificationId)||v.id===v.verificationId||!image(v.image)||!uuid(v.expectedBackupId)||!digest(v.expectedManifestSha256)||!count(v.expectedFiles)||!number(v.jobBudgetBytes)||v.jobBudgetBytes<4194304||v.jobBudgetBytes>32*1024**3||!['running','completed','failed','interrupted'].includes(String(v.state))||v.stage!==(v.state==='running'?'checking-archive-and-restoring':v.state)||!time(v.createdAt)||!time(v.updatedAt)||Number(v.updatedAt)<Number(v.createdAt))return fail();
  const source=sources.get(v.verificationId);if(!source||source.backupId!==v.expectedBackupId||source.image!==v.image||source.files!==v.expectedFiles)return fail();
  if(['failed','interrupted'].includes(String(v.state))?typeof v.errorCode!=='string'||!(/^[A-Z_]{1,128}$/).test(v.errorCode):v.errorCode!==null)return fail();
  if(v.state==='completed'){const r=v.receipt;if(!closed(r,'operation,backupId,authenticatedManifestSha256,files')||r.operation!=='restored-and-verified'||r.backupId!==v.expectedBackupId||r.authenticatedManifestSha256!==v.expectedManifestSha256||r.files!==v.expectedFiles)return fail();}else if(v.receipt!==null)return fail();
  jobs.add(v.id);
 }
 return value as unknown as ExternalContext;
}
