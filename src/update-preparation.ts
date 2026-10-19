// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
export const preparationStates=['awaiting-app-close','running','prepared','failed','interrupted','cancelled-before-start'] as const;
export const preparationPhases=['validating-inputs','original-backup','preparing-plan','restoring-candidate','stopping-candidate','checkpoint','full-qualification','registered'] as const;
export interface PreparationReceipt{id:string;installationId:string;state:typeof preparationStates[number];phase:typeof preparationPhases[number];code:string|null;referenceId:string|null;updatedAt:number;canCancel:boolean;canRetry:boolean}
export interface PreparationStatus{controllerBlocked:boolean;canOpenManager:boolean;job:PreparationReceipt|null}
export interface PreparationInspection{available:boolean;code:string|null;version:string|null;changedSchema:boolean|null}
export interface PreparationInput{selectionToken:string;releaseFolder:string;keyFile:string}
const object=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
const exact=(v:Record<string,unknown>,keys:string[])=>Object.keys(v).length===keys.length&&keys.every(k=>Object.hasOwn(v,k));
export const preparationUUID=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(v);
const safeCode=(v:unknown)=>v===null||typeof v==='string'&&/^[A-Z][A-Z0-9_]{0,127}$/.test(v);
const fail=():never=>{throw Error('MANAGER_PROTOCOL');};
export function parsePreparationReceipt(v:unknown):PreparationReceipt{
 if(!object(v)||!exact(v,['id','installationId','state','phase','code','referenceId','updatedAt','canCancel','canRetry'])||!preparationUUID(v.id)||!preparationUUID(v.installationId)||!preparationStates.includes(v.state as PreparationReceipt['state'])||!preparationPhases.includes(v.phase as PreparationReceipt['phase'])||!safeCode(v.code)||!(v.referenceId===null||preparationUUID(v.referenceId))||typeof v.updatedAt!=='number'||!Number.isSafeInteger(v.updatedAt)||v.updatedAt<0||v.updatedAt>8.64e15||typeof v.canCancel!=='boolean'||typeof v.canRetry!=='boolean'||v.canCancel&&v.state!=='awaiting-app-close')return fail();
 if(v.canRetry!==false)return fail();
 if(v.state==='prepared'&&(v.phase!=='registered'||!preparationUUID(v.referenceId)||v.code!==null||v.canCancel))return fail();
 if((v.state==='awaiting-app-close'||v.state==='running')&&(v.code!==null||v.referenceId!==null))return fail();
 if(v.state==='cancelled-before-start'&&(v.phase!=='validating-inputs'||v.referenceId!==null||v.code!==null||v.canCancel))return fail();
 if((v.state==='failed'||v.state==='interrupted')&&(v.referenceId!==null||v.code===null))return fail();
 return v as unknown as PreparationReceipt;
}
export function parsePreparationStatus(v:unknown):PreparationStatus{
 if(!object(v)||!exact(v,['controllerBlocked','canOpenManager','job'])||typeof v.controllerBlocked!=='boolean'||typeof v.canOpenManager!=='boolean'||v.controllerBlocked&&v.canOpenManager)return fail();
 const job=v.job===null?null:parsePreparationReceipt(v.job);if(v.controllerBlocked&&!job||job&&['awaiting-app-close','running','interrupted'].includes(job.state)&&!v.controllerBlocked)return fail();
 return {controllerBlocked:v.controllerBlocked,canOpenManager:v.canOpenManager,job};
}
export function parsePreparationInspection(v:unknown):PreparationInspection{
 if(!object(v)||!exact(v,['available','code','version','changedSchema'])||typeof v.available!=='boolean'||!safeCode(v.code)||!(v.version===null||typeof v.version==='string'&&v.version.length>0&&v.version.length<=128)||!(v.changedSchema===null||typeof v.changedSchema==='boolean'))return fail();
 return v as unknown as PreparationInspection;
}
export function validPreparationPath(v:unknown):v is string{return typeof v==='string'&&v.length>0&&v.length<=4096&&!/[\u0000-\u001f\u007f]/.test(v)&&(/^\//.test(v)||/^[A-Za-z]:[\\/]/.test(v)||/^\\\\/.test(v));}
function validateInput(input:PreparationInput){if(!preparationUUID(input.selectionToken)||!validPreparationPath(input.releaseFolder)||!validPreparationPath(input.keyFile))fail();}
async function statusCall(command:string,id?:string){if(id!==undefined&&!preparationUUID(id))fail();const result=parsePreparationStatus(await invoke(command,{input:{id:id??null}}));if(id&&result.job&&result.job.id!==id)fail();return result;}
export const preparationClient={
 native:isTauri(),
 openManager:async()=>{await invoke('manager_update_preparation_open_manager',{});},
 startup:async()=>parsePreparationStatus(await invoke('manager_update_preparation_startup',{})),
 status:(id?:string)=>statusCall('manager_update_preparation_status',id),
 cancel:async(id:string)=>{if(!preparationUUID(id))fail();const result=parsePreparationStatus(await invoke('manager_update_preparation_cancel',{input:{id}}));if(result.job&&result.job.id!==id)fail();return result;},
 reconcile:async(id:string)=>{if(!preparationUUID(id))fail();const result=parsePreparationStatus(await invoke('manager_update_preparation_reconcile',{input:{id}}));if(result.job&&result.job.id!==id)fail();return result;},
 chooseRelease:async(selectionToken:string)=>{if(!preparationUUID(selectionToken))fail();const value=await invoke('manager_update_preparation_choose_release',{selectionToken});if(value!==null&&!validPreparationPath(value))fail();return value as string|null;},
 inspect:async(input:PreparationInput)=>{validateInput(input);return parsePreparationInspection(await invoke('manager_update_preparation_inspect',{input}));},
 dispatch:async(input:PreparationInput&{preserveData:true;externalWritersQuiesced:true;closeAppAndPrepare:true})=>{validateInput(input);if(input.preserveData!==true||input.externalWritersQuiesced!==true||input.closeAppAndPrepare!==true)fail();return parsePreparationReceipt(await invoke('manager_update_preparation_dispatch',{input}));}
};
export const preparationPhaseLabel:Record<PreparationReceipt['phase'],string>={'validating-inputs':'선택한 자료 확인','original-backup':'원래 전시 백업','preparing-plan':'업데이트 계획 준비','restoring-candidate':'별도 공간에 복원','stopping-candidate':'복원된 후보 정지',checkpoint:'복구 지점 보관','full-qualification':'전체 복구·업데이트 검사',registered:'적용 자료 등록'};
export const preparationStateLabel:Record<PreparationReceipt['state'],string>={'awaiting-app-close':'앱을 닫으면 준비를 시작합니다',running:'업데이트 준비 중',prepared:'업데이트 적용 준비가 끝났습니다',failed:'업데이트 준비를 마치지 못했습니다',interrupted:'업데이트 준비가 중단됐습니다','cancelled-before-start':'시작 전에 준비를 취소했습니다'};
// Presentation-only, consumed once after native startup permits ordinary Manager effects.
const resumeViewKey='exhibitos.manager.preparation.resumeView';
let memoryResumeView=false;
export function rememberPreparationView(){memoryResumeView=true;try{localStorage.setItem(resumeViewKey,'updates');}catch{/* In-process navigation still works; this is never an execution permit. */}}
export function consumePreparationView(){
 let resume=memoryResumeView;memoryResumeView=false;
 try{if(localStorage.getItem(resumeViewKey)==='updates'){localStorage.removeItem(resumeViewKey);resume=true;}}catch{/* A storage failure never blocks Manager or grants authority. */}
 if(resume)window.location.hash='#updates';
}
export function preparationGuidance(code:string|null){
 if(code==='UPDATE_PREPARATION_START_REQUIRED')return '백업 확인을 위해 서버가 켜져 있어야 해요. Manager에서 서버 시작을 누른 뒤 업데이트 자료를 다시 확인해 주세요. 현재 전시와 백업은 변경하지 않았어요.';
 if(code?.includes('RUNTIME')||code?.includes('PYTHON'))return '이 Manager에 필요한 준비 실행 도구가 없습니다. 도구가 포함된 검증판을 사용하세요. 준비 완료로 처리하지 않았습니다.';
 if(code?.includes('KEY'))return '선택한 백업 키 파일을 확인하고 다시 검사하세요. 키 내용은 화면에 표시하지 않습니다.';
 return '선택한 자료와 현재 기록을 확인하세요. 원본·백업·기존 후보를 보존하고, 원인을 확인한 뒤 다시 진행하세요.';
}
