// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {client,managerError,backupStageLabel,restorationStageLabel,backupStages,restorationStages,type BackupJob,type RestorationJob,type MaintenanceContext,type ManagerError} from './lifecycle';
export function MaintenanceCancellation({native,available,onActivity}:{native:boolean;available:boolean;onActivity:(active:boolean)=>void}){
 const [context,setContext]=useState<MaintenanceContext|null>(null),[error,setError]=useState<ManagerError|null>(null),[consent,setConsent]=useState(false),[pending,setPending]=useState(false);
 const epoch=useRef(0);
 const live=useRef(true),requesting=useRef(false),currentId=useRef<string|null>(null);
 useEffect(()=>{
  live.current=true;let timer:ReturnType<typeof setTimeout>;
  const poll=async()=>{
   if(native&&available&&!requesting.current){const generation=epoch.current;try{const value=await client.maintenanceContext();if(live.current&&generation===epoch.current){if(currentId.current!==value?.id){setConsent(false);currentId.current=value?.id??null;}setContext(value);setError(null);onActivity(value?.state==='running'||value?.state==='requested');}}
   catch(e){const result=managerError(e);if(live.current&&generation===epoch.current&&result.code!=='BUSY'){setError(result);setContext(null);setConsent(false);onActivity(true);}}}
   if(live.current)timer=setTimeout(()=>void poll(),1000);
  };void poll();return()=>{live.current=false;clearTimeout(timer);};
 },[native,available,onActivity]);
 async function cancel(){
  if(!context||context.state!=='running'||!consent||requesting.current||!native||!available)return;
  epoch.current++;requesting.current=true;setPending(true);setError(null);
  try{const result=await client.cancelMaintenance({kind:context.kind,targetId:context.id,preserveCandidates:consent});if(live.current){setContext(result);onActivity(true);setConsent(false);}}
  catch(e){if(live.current){setError(managerError(e));setConsent(false);}}
  finally{requesting.current=false;if(live.current)setPending(false);}
 }
 const stageLabel=context?.kind==='backup'&&backupStages.includes(context.stage as BackupJob['stage'])?backupStageLabel(context.stage as BackupJob['stage']):context?.kind==='restoration'&&restorationStages.includes(context.stage as RestorationJob['stage'])?restorationStageLabel(context.stage as RestorationJob['stage']):'단계 확인 필요';
 const waiting=context?.state==='requested',confirmed=context?.state==='confirmed';
 return <section className="panel maintenance-cancellation" aria-label="진행 중 백업·복원 취소"><h2>진행 중 백업·복원 취소</h2><p>후보와 원본 데이터를 보존하며 작업을 중단합니다. 서버를 자동 재개하지 않습니다.</p>
 {error&&<div className="alert" role="alert"><p>{error.guidance}</p><p className="code">확인 코드: {error.code}</p></div>}
 <p role="status" aria-live="polite">{pending?'취소 요청을 저장하고 있습니다.':error?'현재 작업과 정지 여부를 확인할 수 없습니다.':waiting?'취소를 요청했습니다. 실제 helper와 새 복원 서버의 정지 확인을 기다립니다.':confirmed?'취소를 확인했습니다. 후보와 데이터는 보존됐으며 서버를 자동 재개하지 않았습니다.':context?.state==='uncertain'?'정지 확인이 불확실합니다. helper와 실제 서버 상태를 확인하세요.':context?.state==='interrupted'?'앱 작업이 중단됐습니다. 취소 완료가 확인되지 않았습니다.':context?.state==='running'?`${context.kind==='backup'?'백업 생성':'새 설치 복원'} 진행 중 · ${stageLabel}`:'현재 취소할 백업·복원 작업이 없습니다.'}</p>
 {(context?.state==='running'||waiting)&&<><p>이미지 저장·불러오기와 파일 검사 중에는 현재 명령이 종료될 때까지 기다릴 수 있습니다. 창을 닫거나 취소 요청을 저장한 것만으로 정지가 확인되지는 않습니다.</p><label className="maintenance-ack"><input type="checkbox" checked={consent} disabled={pending||waiting} onChange={e=>setConsent(e.target.checked)}/>실패 후보와 데이터를 보존하고 서버를 자동 재개하지 않는 것에 동의합니다.</label><button disabled={!native||!available||!!error||!consent||pending||waiting} onClick={()=>void cancel()}>작업 취소 요청</button></>}
 </section>;
}
