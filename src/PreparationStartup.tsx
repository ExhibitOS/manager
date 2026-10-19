// SPDX-License-Identifier: Apache-2.0
import {createContext,useCallback,useContext,useEffect,useRef,useState,type ReactNode} from 'react';
import {AppearanceControl} from './AppearanceControl';
import {preparationClient,preparationGuidance,rememberPreparationView,consumePreparationView,preparationPhaseLabel,preparationStateLabel,type PreparationReceipt,type PreparationStatus} from './update-preparation';
import './style.css';
const PreparationBoundary=createContext<{block:(job:PreparationReceipt|null)=>void;latest:PreparationReceipt|null}>({block:()=>{throw Error('MANAGER_PROTOCOL');},latest:null});
export const usePreparationBoundary=()=>useContext(PreparationBoundary).block;
export const usePreparationRecord=()=>useContext(PreparationBoundary).latest;
export function PreparationStartup({children}:{children:ReactNode}){
 const [result,setResult]=useState<PreparationStatus|null>(null),[error,setError]=useState(false),[pending,setPending]=useState(false);
 const live=useRef(true),working=useRef(false),epoch=useRef(0);
 const check=useCallback(async(action:'startup'|'status'|'cancel'|'reconcile'='startup',id?:string)=>{
  if(working.current)return;working.current=true;setPending(true);setError(false);const run=++epoch.current;
  try{
   const value=action==='startup'?await preparationClient.startup():action==='status'?await preparationClient.status(id):action==='cancel'?await preparationClient.cancel(id!):await preparationClient.reconcile(id!);
   // Only the native startup check can permit mounting Controller-backed Manager effects.
   const verified=value.controllerBlocked||action==='startup'?value:await preparationClient.startup();
   if(live.current&&epoch.current===run){if(!verified.controllerBlocked&&!verified.canOpenManager)consumePreparationView();setResult(verified);}
  }catch{if(live.current&&epoch.current===run)setError(true);}
  finally{if(live.current&&epoch.current===run){working.current=false;setPending(false);}}
 },[]);
 useEffect(()=>{live.current=true;if(preparationClient.native)void check();return()=>{live.current=false;epoch.current++;};},[check]);
 const block=useCallback((job:PreparationReceipt|null)=>{epoch.current++;working.current=false;setPending(false);setError(false);setResult(job?{controllerBlocked:true,canOpenManager:false,job}:null);if(!job)void check();},[check]);
 async function openManager(){
  if(working.current||!result?.canOpenManager||result.controllerBlocked)return;
  working.current=true;setPending(true);setError(false);
  try{rememberPreparationView();await preparationClient.openManager();}catch{if(live.current){setError(true);working.current=false;setPending(false);}}
  // Successful native dispatch restarts this process. Never mount App in the waiting process.
 }
 const job=result?.job;
 useEffect(()=>{
  if(!result?.controllerBlocked||!job||!['running','awaiting-app-close'].includes(job.state))return;
  const timer=window.setInterval(()=>void check('status',job.id),3000);return()=>window.clearInterval(timer);
 },[result?.controllerBlocked,job?.id,job?.state,check]);
 if(!preparationClient.native||result&&!result.controllerBlocked&&!result.canOpenManager&&!error)return <PreparationBoundary.Provider value={{block,latest:result?.job??null}}>{children}</PreparationBoundary.Provider>;
 return <main><header className="page-header"><h1>업데이트 준비</h1><AppearanceControl/></header><section className="panel" aria-label="업데이트 준비 상태">
  <div className="lifecycle-feedback" role="status" aria-live="polite" aria-atomic="true">
   <strong>{job?preparationStateLabel[job.state]:result?.canOpenManager?'Manager를 다시 열 수 있습니다':'저장된 준비 기록을 확인하고 있습니다'}</strong>
   {(pending||job?.state==='running')&&<progress aria-label={job?preparationPhaseLabel[job.phase]:'준비 기록 응답 대기'}/>}
   {job&&<p>{preparationPhaseLabel[job.phase]}</p>}
  </div>
  {job?.state==='awaiting-app-close'&&<p>이 앱을 완전히 종료하면 별도 준비 작업이 시작됩니다. 다시 열면 실제 단계와 결과를 확인할 수 있습니다. 준비 중 전시가 중단될 수 있습니다.</p>}
  {result?.canOpenManager&&<p>일반 관리 창을 다시 열 준비가 됐습니다. Manager로 돌아가기를 선택하세요. 업데이트 화면이 열리지 않으면 업데이트 메뉴를 선택하세요.</p>}
  {job?.state==='prepared'&&<p>준비는 업데이트 적용과 다릅니다. 기록 확인 후 Manager에서 적용 자료를 다시 확인하세요.</p>}
  {job&&['failed','interrupted'].includes(job.state)&&<p>{preparationGuidance(job.code)}</p>}
  {error&&<div className="alert" role="alert">준비 기록을 확인하지 못했습니다. 일반 관리 작업은 열지 않았습니다. 상태를 다시 확인하세요.</div>}
  <div className="button-row"><button disabled={pending} onClick={()=>void check('status',job?.id)}>상태 다시 확인</button>
   {job?.canCancel&&<button disabled={pending} onClick={()=>void check('cancel',job.id)}>시작 전 준비 취소</button>}
   {result?.canOpenManager&&<button className="primary" disabled={pending} onClick={()=>void openManager()}>Manager로 돌아가기</button>}
   {job&&result?.controllerBlocked&&!['awaiting-app-close','running'].includes(job.state)&&<button disabled={pending} onClick={()=>void check('reconcile',job.id)}>준비 기록 확인 후 Manager 열기</button>}
  </div>
  <p className="note">준비 작업을 임의로 재시작하거나 실행 중인 작업을 취소하지 않습니다. 현재 단계는 완료율이나 전시의 정상 응답을 보장하지 않습니다.</p>
  {job&&<details><summary>진단 정보</summary><p className="code">작업 ID: {job.id}</p>{job.code&&<p className="code">확인 코드: {job.code}</p>}<p>최근 기록: {new Date(job.updatedAt).toLocaleString('ko-KR')}</p></details>}
 </section></main>;
}
