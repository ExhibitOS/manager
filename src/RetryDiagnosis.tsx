// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {managerError,installationLabel,type InstallationContext,type MaintenanceRetry,type ManagerError,type RetryDiagnosis,type RetryDiagnosisInput,type RetryRecoveryReceipt} from './lifecycle';
interface Props{native:boolean;active:boolean;installation:InstallationContext|null;history:MaintenanceRetry[];historyError:ManagerError|null;diagnose(input:RetryDiagnosisInput):Promise<RetryDiagnosis>;reconcile(input:RetryDiagnosisInput):Promise<RetryRecoveryReceipt>}
const labels:Record<RetryDiagnosis['outcome'],string>={'child-found':'예약과 일치하는 새 작업을 찾았습니다.','no-child-created':'새 작업이 생성되지 않은 조건을 확인했습니다.','already-linked':'이미 새 작업이 연결돼 있습니다.','unproven':'준비 증거가 없어 연결을 추측하지 않습니다.','candidate-incomplete':'불완전한 후보를 보존하고 별도 복구가 필요합니다.','child-journal-missing':'새 작업 기록이 없습니다. 후보와 연결을 보존하세요.','inventory-changed':'원래 공간의 후보 목록이 달라졌습니다.','destination-changed':'목적지 공간의 내용이 달라졌습니다.'};
export function RetryDiagnosisPanel({native,active,installation,history,historyError,diagnose,reconcile}:Props){
 const [selected,setSelected]=useState(''),[result,setResult]=useState<RetryDiagnosis|null>(null),[receipt,setReceipt]=useState<RetryRecoveryReceipt|null>(null),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[error,setError]=useState<ManagerError|null>(null),[hashes,setHashes]=useState<Record<string,string>>({});
 const live=useRef(true),working=useRef(false),errorElement=useRef<HTMLDivElement>(null);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 useEffect(()=>{let cancelled=false;void Promise.all((installation?.installations??[]).map(async r=>{const bytes=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(r.path));return [r.id,Array.from(new Uint8Array(bytes),b=>b.toString(16).padStart(2,'0')).join('')] as const;})).then(entries=>{if(!cancelled)setHashes(Object.fromEntries(entries));}).catch(()=>{if(!cancelled)setHashes({});});return()=>{cancelled=true;};},[installation]);
 const choices=history.filter(r=>r.state!=='completed'),audit=choices.find(r=>r.id===selected);
 const destination=audit&&installation?.installations.find(r=>r.available&&hashes[r.id]===audit.destinationRootSha256&&(audit.kind==='backup'?r.id===installation.activeId:r.id!==installation.activeId));
 const auditVersion=audit?JSON.stringify(audit):'';
 useEffect(()=>{setResult(null);setAck(false);if(!auditVersion)setReceipt(null);},[auditVersion]);
 const disabled=!native||active||pending||!!historyError||!installation||!audit||!destination;
 function clear(){setResult(null);setReceipt(null);setAck(false);setError(null);}
 async function run(recover:boolean){
  if(disabled||working.current||!audit||!destination||recover&&(!result?.canReconcile||result.retryId!==audit.id||!ack))return;
  working.current=true;setPending(true);setError(null);setReceipt(null);setAck(false);
  const input={retryId:audit.id,destinationId:audit.kind==='backup'?null:destination.id,preserveCandidates:recover};
  try{if(recover){const value=await reconcile(input);if(result&&(value.outcome!==result.outcome||value.outcome==='child-found'&&value.newJobId!==result.newJobId))throw Error('MANAGER_PROTOCOL');if(live.current){setReceipt(value);setResult(null);}}else{setResult(null);const value=await diagnose(input);if(value.targetId!==audit.targetId||value.kind!==audit.kind)throw Error('MANAGER_PROTOCOL');if(live.current)setResult(value);}}
  catch(e){if(live.current){setError(managerError(e));setResult(null);}}
  finally{working.current=false;if(live.current)setPending(false);}
 }
 if(!choices.length&&!historyError)return null;
 return <section className="panel backup-verification retry-diagnosis" aria-labelledby="retry-diagnosis-heading"><p className="eyebrow">CHECK THE EVIDENCE · KEEP ALL DATA</p><h2 id="retry-diagnosis-heading">재시도 연결 진단</h2><p>새 작업 ID를 기록하기 전에 앱이 종료됐다면 준비·예약·후보를 확인합니다. 진단과 기록 복구는 전시·helper를 실행하거나 데이터를 복원하지 않습니다.</p>
 {historyError&&<p role="alert">재시도 기록을 읽을 수 없습니다. 원래 기록과 후보를 보존하고 CLI 진단을 확인하세요.</p>}
 <label htmlFor="diagnosis-target">진단할 재시도 기록</label><select id="diagnosis-target" value={audit?selected:''} disabled={!native||active||pending||!!historyError} onChange={e=>{setSelected(e.target.value);clear();}}><option value="">재시도 기록을 선택하세요</option>{choices.map(r=><option key={r.id} value={r.id}>{r.kind==='backup'?'백업':'복원'} · {r.id}</option>)}</select>
 {audit&&<><p className="code">원래 작업: {audit.targetId}</p><p>{destination?`확인할 목적지: ${installationLabel(destination)} · ${destination.id}`:'등록된 목적지를 확인하지 못했습니다. 경로와 후보를 유지하고 CLI 진단을 사용하세요.'}</p></>}
 <button disabled={disabled} onClick={()=>void run(false)}>{pending?'기록 확인 중…':'기록을 변경하지 않고 진단'}</button>
 {result&&<div className="verification-result" role="status"><h3>{labels[result.outcome]}</h3>{result.newJobId&&<p className="code">예약·연결 작업: {result.newJobId}</p>}{result.canReconcile?<><p>{result.outcome==='child-found'?'명시적 기록 복구는 이 작업을 연결하고 중단 상태로 유지합니다. 목적지에서 해당 작업과 helper 상태를 별도로 확인하세요.':'명시적 기록 복구는 증거를 다시 검사하고 원래 작업을 새 작업으로 재시도할 수 있게 합니다. 재시도는 자동 실행하지 않습니다.'}</p><label className="backup-ack"><input type="checkbox" checked={ack} disabled={active||pending} onChange={e=>setAck(e.target.checked)}/><span>원래 기록·후보·데이터를 보존하고 검증된 재시도 연결 기록만 복구하는 데 동의합니다.</span></label><button disabled={disabled||!ack} onClick={()=>void run(true)}>증거를 다시 검사하고 연결 기록 복구</button></>:<p>자동으로 차단을 풀거나 작업 ID를 추측하지 않습니다. 표시된 기록과 후보를 보존하세요.</p>}</div>}
 {receipt&&<div role="status"><h3>연결 기록 복구 완료</h3><p className="code">진단 기록: {receipt.diagnosisId}</p><p>{receipt.outcome==='child-found'?'발견된 새 작업을 중단 상태로 연결했습니다. 해당 공간에서 작업 상태를 확인하세요.':'새 작업이 생성되지 않은 증거를 저장했습니다. 상태 확인 후 원래 작업의 새 재시도를 직접 선택하세요.'}</p><p>데이터 복원·helper 정지·전시 재개는 실행하지 않았습니다.</p>{installation?.errorCode&&<p>일반 상태 조회가 계속 실패하면 앱을 닫고 다시 열어 상태를 확인하세요. 기존 후보는 지우지 마세요.</p>}</div>}
 {error&&<div className="alert" role="alert" tabIndex={-1} ref={errorElement}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>완료 여부가 확인되지 않았습니다. 기록과 후보를 보존하고 다시 진단하세요.</p></div>}
 </section>;
}
