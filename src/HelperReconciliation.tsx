// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {managerError,type BackupJob,type RestorationContext,type ManagerError,type ReconciliationJob,type ReconciliationInput,type ReconciliationReceipt} from './lifecycle';
interface Props{native:boolean;active:boolean;backupJobs:BackupJob[];restoration:RestorationContext|null;targetError:ManagerError|null;history:ReconciliationJob[];historyError:ManagerError|null;reconcile(input:ReconciliationInput):Promise<ReconciliationReceipt>}
export function HelperReconciliation({native,active,backupJobs,restoration,targetError,history,historyError,reconcile}:Props){
 const candidates:[ReconciliationInput['kind'],string][]=backupJobs.filter(j=>['failed','interrupted'].includes(j.state)).map(j=>['backup',j.id]);
 if(restoration?.job&&['failed','interrupted'].includes(restoration.job.state))candidates.push(['restoration',restoration.job.id]);
 const [target,setTarget]=useState(''),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[result,setResult]=useState<ReconciliationReceipt|null>(null),[error,setError]=useState<ManagerError|null>(null);
 const working=useRef(false),live=useRef(true),errorElement=useRef<HTMLDivElement>(null);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 const selected=candidates.find(([kind,id])=>`${kind}:${id}`===target),disabled=!native||active||pending||!!targetError||!!historyError;
 async function submit(event:React.SubmitEvent<HTMLFormElement>){
  event.preventDefault();if(working.current||disabled||!ack||!selected)return;working.current=true;setPending(true);setResult(null);setError(null);
  try{const receipt=await reconcile({kind:selected[0],targetId:selected[1],preserveCandidates:true});if(live.current)setResult(receipt);}catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current){setPending(false);setAck(false);}}
 }
 if(!candidates.length&&!history.length&&!targetError&&!historyError)return null;
 return <section className="panel helper-reconciliation" aria-labelledby="helper-heading">
  <p className="eyebrow">RECOVER WITHOUT ERASING</p><h2 id="helper-heading">중단된 작업의 helper 확인</h2>
  <p>백업·복원 실패 후 남아 있는 보조 실행을 확인하고 해당 작업의 helper만 정지합니다. 사본·키·부분 DB·volume을 삭제하지 않으며 container 삭제 명령을 보내지 않습니다.</p>
  <p id="helper-scope" className="note">원래 작업을 성공으로 바꾸거나 서버를 재개하지 않습니다. 복원 재시도는 관리 공간 선택에서 새 공간을 만든 뒤 진행하세요. 실행 중인 작업 취소 기능은 아직 제공하지 않습니다. 엔진의 --rm 설정이 있는 helper는 정지 후 자동 제거될 수 있지만 작업 공간과 데이터는 보존됩니다.</p>
  <form onSubmit={event=>void submit(event)} aria-describedby="helper-scope"><fieldset disabled={disabled||candidates.length===0}><legend>실패·중단 작업 선택</legend>
   <label htmlFor="helper-target">현재 공간의 작업</label><select id="helper-target" value={selected?target:''} onChange={event=>{setTarget(event.target.value);setAck(false);setResult(null);setError(null);}}>
    <option value="">확인할 작업을 선택하세요</option>{candidates.map(([kind,id])=><option key={`${kind}:${id}`} value={`${kind}:${id}`}>{kind==='backup'?'백업':'복원'} · {id}</option>)}
   </select>
   <label className="backup-ack"><input type="checkbox" checked={ack} onChange={event=>setAck(event.target.checked)}/><span>후보 데이터를 보존하고 해당 helper만 정지하며 서버를 자동 재개하지 않는 것을 확인했습니다.</span></label>
  </fieldset><button type="submit" disabled={disabled||!ack||!selected}>{pending?'helper 상태 확인 중…':'해당 helper 확인·정지'}</button></form>
  {!candidates.length&&<p className="note">확인 가능한 실패·중단 작업이 없습니다.</p>}
  {(targetError||historyError)&&<div role="alert" className="alert"><strong>작업 또는 확인 기록을 읽을 수 없습니다.</strong><p>상태를 다시 확인하기 전에는 정지 명령을 실행하지 않습니다.</p></div>}
  {pending&&<div role="status"><p>소유권과 실제 정지 상태를 확인합니다. 다른 작업은 기다려 주세요.</p><progress aria-label="helper 정지 여부 확인 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>정지 완료로 표시하지 않습니다. 원본·후보와 원래 작업 기록은 보존됩니다.</p></div>}
  {result&&<div role="status" className="verification-result"><h3>helper {result.helperState==='absent'?'부재':'정지'} 확인</h3><p>해당 helper의 실행이 남아 있지 않은 것을 확인했습니다. 원래 백업·복원은 완료로 바꾸지 않으며 서버는 자동 재개하지 않습니다.</p><p className="code">작업 ID: {result.targetId}</p></div>}
  <div role="region" aria-label="helper 확인 기록"><h3>확인 기록</h3>{history.length?<ol className="job-list">{[...history].reverse().slice(0,10).map(job=><li key={job.id}><strong>{job.kind==='backup'?'백업':'복원'} helper</strong><span>{job.state==='completed'?job.helperState==='absent'?'부재 확인':'정지 확인':job.state==='checking'?'확인 중':job.state==='interrupted'?'확인 중단':'확인 실패'}</span><time>{new Date(job.updatedAt).toLocaleString('ko-KR')}</time><p className="code">대상 작업: {job.targetId}</p>{job.errorCode&&<p className="code">확인 코드: {job.errorCode}</p>}</li>)}</ol>:<p className="note">저장된 확인 기록이 없습니다.</p>}</div>
 </section>;
}
