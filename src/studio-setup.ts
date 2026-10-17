// SPDX-License-Identifier: Apache-2.0
import type {ManagerClient,Job} from './lifecycle';
export type StudioSetupStage='checking'|'installing'|'starting'|'opening';
export const studioSetupLabel:Record<StudioSetupStage,string>={checking:'내 컴퓨터의 준비 상태를 확인하고 있어요.',installing:'전시를 설치하고 있어요.',starting:'전시를 시작하고 응답을 확인하고 있어요.',opening:'준비된 Studio를 열고 있어요.'};
type SetupClient=Pick<ManagerClient,'installations'|'status'|'engines'|'install'|'action'|'openOex'>;
/** Only runs after a user action; preserves the existing native writer and readiness checks. */
export async function prepareStudio(api:SetupClient,stage:(value:StudioSetupStage)=>void):Promise<void>{
 stage('checking');
 const selection=await api.installations();
 const sameSelection=async()=>{const current=await api.installations();if(current.selectionToken!==selection.selectionToken||current.activeId!==selection.activeId||current.errorCode)throw {code:'INSTALLATION_CHANGED',guidance:'선택한 전시가 바뀌어 준비를 멈췄어요. 현재 전시를 확인한 뒤 다시 시작하세요.'};};
 if(selection.errorCode||!selection.installations.some(root=>root.id===selection.activeId&&root.available))throw {code:'INSTALLATION_UNAVAILABLE',guidance:'선택한 전시 저장 위치를 확인하세요. 기존 데이터는 변경하지 않았어요.'};
 let status=await api.status();
 if(status.activeJob)throw {code:'BUSY',guidance:'진행 중인 작업이 있어요. 작업 기록에서 완료 여부를 확인한 뒤 다시 열어 주세요.'};
 const completed=(job:Job)=>{if(job.state!=='completed')throw {code:job.errorCode??(job.state==='running'?'BUSY':'JOB_FAILED'),guidance:job.guidance??'준비가 아직 완료되지 않았어요. 작업 기록을 확인한 뒤 해당 단계부터 다시 시도하세요.'};};
 if(!status.installed||!status.readiness.ready){
  if(!(await api.engines()).some(engine=>engine.available))throw {code:'ENGINE_UNAVAILABLE',guidance:'실행 도구를 준비하거나 시작한 뒤 다시 시도하세요. 기존 전시는 그대로 있어요.'};
  if(!status.installed){await sameSelection();stage('installing');completed(await api.install());status=await api.status();if(!status.installed)throw {code:'INSTALLATION_NOT_READY',guidance:'설치 완료를 확인하지 못했어요. 작업 기록을 확인한 뒤 다시 시도하세요.'};}
  if(!status.readiness.ready){if(status.activeJob)throw {code:'BUSY',guidance:'진행 중인 작업이 있어요. 작업 기록에서 완료 여부를 확인하세요.'};await sameSelection();stage('starting');completed(await api.action('start'));status=await api.status();}
 }
 if(!status.readiness.ready||status.activeJob)throw {code:'RUNTIME_NOT_READY',guidance:'전시가 아직 응답할 준비를 마치지 못했어요. 전시 상태와 작업 기록을 확인한 뒤 다시 시도하세요.'};
 await sameSelection();stage('opening');await api.openOex();
}
