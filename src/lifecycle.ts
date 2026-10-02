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
export interface ManagerClient{native:boolean;status():Promise<Status>;engines():Promise<EngineProbe[]>;jobs():Promise<Job[]>;logs():Promise<LogEvent[]>;install():Promise<Job>;action(action:Action):Promise<Job>;open():Promise<void>}
export const client:ManagerClient={native:isTauri(),status:async()=>parseStatus(await invoke('manager_status')),engines:async()=>parseEngines(await invoke('manager_detect')),jobs:async()=>{const v=await invoke('manager_jobs');if(!Array.isArray(v)||v.length>200)throw Error('MANAGER_PROTOCOL');return v.map(parseJob);},logs:async()=>parseLogs(await invoke('manager_logs')),install:async()=>parseJob(await invoke('manager_install')),action:async(action)=>{if(!['start','stop','restart','retry'].includes(action))throw Error('MANAGER_ACTION');return parseJob(await invoke('manager_action',{action}));},open:async()=>{await invoke('manager_open_exhibition');}};
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
