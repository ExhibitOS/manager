// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
export type Action='start'|'stop'|'restart'|'retry';
export interface Job{id:string;action:'install'|Action;state:'running'|'completed'|'failed'|'interrupted';attempt:number;progress:number;createdAt:number;updatedAt:number;errorCode:string|null;guidance:string|null}
export interface EngineProbe{kind:string;installed:boolean;available:boolean;engineVersion:string|null;composeVersion:string|null;errorCode:string|null;guidance:string|null}
export interface Status{installed:boolean;bundleId:string|null;version:string|null;state:string;services:{name:string;state:string;health:string|null}[];readiness:{ready:boolean;version:string|null;protocolVersion:string|null;errorCode:string|null};storage:{usedBytes:number|null;freeBytes:number;minimumFreeBytes:number;quotaBytes?:number};activeJob:Job|null}
export interface LogEvent{at:number;jobId:string|null;code:string;message:string}
export interface ManagerError{code:string;guidance:string}
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
const text=(v:unknown,max=512):v is string=>typeof v==='string'&&v.length<=max;
const optional=(v:unknown)=>v===null||text(v);
const number=(v:unknown):v is number=>typeof v==='number'&&Number.isSafeInteger(v)&&v>=0;
export function parseJob(v:unknown):Job{
 if(!object(v)||!text(v.id,64)||!['install','start','stop','restart','retry'].includes(String(v.action))||!['running','completed','failed','interrupted'].includes(String(v.state))||!number(v.attempt)||!number(v.progress)||v.progress>100||!number(v.createdAt)||!number(v.updatedAt)||!optional(v.errorCode)||!optional(v.guidance))throw Error('MANAGER_PROTOCOL');return v as unknown as Job;
}
export function parseStatus(v:unknown):Status{
 if(!object(v)||typeof v.installed!=='boolean'||!optional(v.bundleId)||!optional(v.version)||!text(v.state,64)||!Array.isArray(v.services)||v.services.length>32||v.services.some(s=>!object(s)||!text(s.name,128)||!text(s.state,64)||!optional(s.health))||!object(v.readiness)||typeof v.readiness.ready!=='boolean'||!optional(v.readiness.version)||!optional(v.readiness.protocolVersion)||!optional(v.readiness.errorCode)||!object(v.storage)||!(v.storage.usedBytes===null||number(v.storage.usedBytes))||!number(v.storage.freeBytes)||!number(v.storage.minimumFreeBytes)||(v.storage.quotaBytes!==undefined&&!number(v.storage.quotaBytes)))throw Error('MANAGER_PROTOCOL');
 if(v.activeJob!==null)parseJob(v.activeJob);return v as unknown as Status;
}
function parseEngines(v:unknown):EngineProbe[]{if(!Array.isArray(v)||v.length>4||v.some(e=>!object(e)||!text(e.kind,32)||typeof e.installed!=='boolean'||typeof e.available!=='boolean'||!optional(e.engineVersion)||!optional(e.composeVersion)||!optional(e.errorCode)||!optional(e.guidance)))throw Error('MANAGER_PROTOCOL');return v as EngineProbe[];}
function parseLogs(v:unknown):LogEvent[]{if(!Array.isArray(v)||v.length>500||v.some(e=>!object(e)||!number(e.at)||!optional(e.jobId)||!text(e.code,128)||!text(e.message,512)))throw Error('MANAGER_PROTOCOL');return v as LogEvent[];}
export function managerError(v:unknown):ManagerError{if(object(v)&&text(v.code,128)&&/^[A-Z_]+$/.test(v.code)&&text(v.guidance))return {code:v.code,guidance:v.guidance};return {code:'MANAGER_CONNECTION',guidance:'실행 상태를 확인할 수 없습니다. 데스크톱 앱을 다시 열고 상태를 확인하세요.'};}
export interface ManagerClient{native:boolean;status():Promise<Status>;engines():Promise<EngineProbe[]>;jobs():Promise<Job[]>;logs():Promise<LogEvent[]>;install():Promise<Job>;action(action:Action):Promise<Job>;open():Promise<void>;verifyBackup(input:VerificationInput):Promise<VerificationReceipt>;createBackup(input:CreationInput):Promise<CreationReceipt>;backupJobs():Promise<BackupJob[]>}
export const client:ManagerClient={native:isTauri(),status:async()=>parseStatus(await invoke('manager_status')),engines:async()=>parseEngines(await invoke('manager_detect')),jobs:async()=>{const v=await invoke('manager_jobs');if(!Array.isArray(v)||v.length>200)throw Error('MANAGER_PROTOCOL');return v.map(parseJob);},logs:async()=>parseLogs(await invoke('manager_logs')),install:async()=>parseJob(await invoke('manager_install')),action:async(action)=>{if(!['start','stop','restart','retry'].includes(action))throw Error('MANAGER_ACTION');return parseJob(await invoke('manager_action',{action}));},open:async()=>{await invoke('manager_open_exhibition');},verifyBackup:async(input)=>{if(!validVerificationInput(input))throw {code:'BACKUP_INPUT_INVALID',guidance:'백업·키의 전체 경로와 실행 패키지 ID를 확인하세요.'};const receipt=parseVerificationReceipt(await invoke('manager_verify_backup',{input}));if(receipt.image!==input.image)throw Error('MANAGER_PROTOCOL');return receipt;},createBackup:async(input)=>{if(!validCreationInput(input))throw {code:'BACKUP_INPUT_INVALID',guidance:'외부 쓰기 중지와 전시 중단을 확인하고 외부 키 경로·고정 이미지 ID를 준비하세요.'};const receipt=parseCreationReceipt(await invoke('manager_create_backup',{input}));if(receipt.image!==input.image)throw Error('MANAGER_PROTOCOL');return receipt;},backupJobs:async()=>{const jobs=await invoke('manager_backup_jobs');if(!Array.isArray(jobs)||jobs.length>1000)throw Error('MANAGER_PROTOCOL');return jobs.map(parseBackupJob);}};
export const statusLabel=(state:string)=>({not_installed:'설치 전',running:'관람 준비 완료',stopped:'정지됨',degraded:'일부 기능 준비 중',runtime_unavailable:'실행 도구 확인 필요'}[state]??'상태 확인 필요');
export const actionLabel=(action:Job['action'])=>({install:'설치',start:'시작',stop:'정지',restart:'재시작',retry:'다시 시도'}[action]);
export function formatBytes(value:number|null){if(value===null)return '측정할 수 없음';if(value<1024)return `${value} B`;if(value<1048576)return `${(value/1024).toFixed(1)} KiB`;if(value<1073741824)return `${(value/1048576).toFixed(1)} MiB`;return `${(value/1073741824).toFixed(1)} GiB`;}

// Unknown provider states remain unverified; display labels do not change readiness.
export const serviceStateLabel=(state:string)=>({running:'실행 중',exited:'정지됨',stopped:'정지됨',created:'시작 전',restarting:'다시 시작 중',paused:'일시 정지',dead:'실행 실패'}[state.toLowerCase()]??'상태 확인 필요');
export const healthLabel=(health:string|null)=>health?({healthy:'응답 정상',unhealthy:'응답 확인 필요',starting:'준비 중'}[health.toLowerCase()]??'응답 확인 필요'):'응답 검사 없음';
export function nextStep(status:Status|null,engineReady:boolean,active:boolean,native:boolean):string{
 if(!native)return '데스크톱 앱을 열면 실행 도구를 확인하고 설치를 시작할 수 있습니다.';
 if(active)return '진행 중인 작업이 끝나면 상태를 다시 확인합니다. 중복 실행은 막혀 있습니다.';
 if(!status)return '상태 다시 확인을 눌러 실행 상태를 확인하세요.';
 if(!engineReady)return '실행 도구를 시작하거나 설치한 뒤 실행 도구 다시 확인을 누르세요.';
 if(!status.installed)return '검증된 설치 패키지를 준비하고 전시 설치를 누르세요.';
 if(status.readiness.ready)return '전시 열기를 눌러 관람을 시작하세요.';
 return status.state==='stopped'?'시작을 눌러 전시 서버를 켜세요.':'작업 기록의 안내를 확인하고 서버 준비 상태를 다시 확인하세요.';
}

export interface VerificationInput{image:string;keyPath:string;sourcePath:string}
export interface VerificationReceipt{id:string;operation:'verified';files:number;image:string;authenticatedManifestSha256:string;at:number}
const digest=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
export function validVerificationInput(value:VerificationInput):boolean{
 return /^sha256:[a-f0-9]{64}$/.test(value.image)&&[value.keyPath,value.sourcePath].every(path=>path.length>0&&path.length<=2048&&![...path].some(c=>c.charCodeAt(0)<32||c.charCodeAt(0)===127||c===',')&&(/^(?:\/|[A-Za-z]:[\\/])/.test(path)));
}
export function parseVerificationReceipt(value:unknown):VerificationReceipt{
 if(!object(value)||Object.keys(value).sort().join(',')!=='at,authenticatedManifestSha256,files,id,image,operation'||!text(value.id,36)||!/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(value.id)||value.operation!=='verified'||!number(value.files)||value.files<1||value.files>1000000||!text(value.image,71)||!/^sha256:[a-f0-9]{64}$/.test(value.image)||!digest(value.authenticatedManifestSha256)||!number(value.at))throw Error('MANAGER_PROTOCOL');
 return value as unknown as VerificationReceipt;
}

export interface CreationInput{image:string;keyPath:string;externalWritersQuiesced:boolean;downtimeAccepted:boolean}
export interface CreationReceipt{id:string;backupId:string;operation:'created-and-authenticated';files:number;image:string;authenticatedManifestSha256:string;writersPaused:true;at:number}
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(v);
export const backupStages=['preflight','saving-images','pausing-writers','encrypting-and-authenticating','copying-authenticated-archive','complete'] as const;
export interface BackupJob{id:string;operation:'create';state:'running'|'completed'|'failed'|'interrupted';stage:typeof backupStages[number];errorCode:string|null;createdAt:number;updatedAt:number}
export function validCreationInput(v:CreationInput):boolean{
 return v.externalWritersQuiesced===true&&v.downtimeAccepted===true&&validVerificationInput({image:v.image,keyPath:v.keyPath,sourcePath:v.keyPath});
}
export function parseCreationReceipt(v:unknown):CreationReceipt{
 if(!object(v)||Object.keys(v).sort().join(',')!=='at,authenticatedManifestSha256,backupId,files,id,image,operation,writersPaused'||!uuid(v.id)||!uuid(v.backupId)||v.operation!=='created-and-authenticated'||v.writersPaused!==true||!number(v.files)||v.files<1||v.files>1000000||!text(v.image,71)||!/^sha256:[a-f0-9]{64}$/.test(v.image)||!digest(v.authenticatedManifestSha256)||!number(v.at))throw Error('MANAGER_PROTOCOL');
 return v as unknown as CreationReceipt;
}
export function parseBackupJob(v:unknown):BackupJob{
 if(!object(v)||Object.keys(v).sort().join(',')!=='createdAt,errorCode,id,operation,stage,state,updatedAt'||!uuid(v.id)||v.operation!=='create'||!['running','completed','failed','interrupted'].includes(String(v.state))||!backupStages.includes(v.stage as BackupJob['stage'])||!(v.errorCode===null||text(v.errorCode,128)&&/^[A-Z_]+$/.test(v.errorCode))||!number(v.createdAt)||!number(v.updatedAt)||v.updatedAt>8640000000000000||v.createdAt>8640000000000000||v.updatedAt<v.createdAt||v.state==='completed'&&(v.stage!=='complete'||v.errorCode!==null)||v.state==='running'&&v.errorCode!==null||['failed','interrupted'].includes(String(v.state))&&v.errorCode===null)throw Error('MANAGER_PROTOCOL');
 return v as unknown as BackupJob;
}
export const backupStageLabel=(stage:BackupJob['stage'])=>({'preflight':'설치와 권한 확인','saving-images':'실행 이미지 보존','pausing-writers':'전시 쓰기 중지','encrypting-and-authenticating':'암호화와 인증 검사','copying-authenticated-archive':'인증 사본 보관','complete':'사본 생성 완료'}[stage]);
