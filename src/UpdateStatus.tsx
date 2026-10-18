// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {BackupToolField,type BackupToolContext} from './BackupToolField';
import {BackupPathField} from './BackupPathField';
import {client,managerError,validVerificationInput,type UpdateRollbackContext,type UpdateRollbackReceipt,type UpdateStatus as Status,type ManagerError,type UpdateCancelReceipt,type UpdateReference,type UpdateApplyReceipt} from './lifecycle';
const labels={prepared:'업데이트 준비 기록',applying:'업데이트 적용 단계 기록',awaiting_health:'새 전시 응답 확인 단계 기록',updated:'업데이트 완료 기록',recovery_required:'원래 전시 복원 필요',restoring:'원래 전시 복원 단계 기록',awaiting_rollback_health:'복원된 전시 응답 확인 단계 기록',rolled_back:'원래 전시 복원 완료 기록'};
export function UpdateStatusPanel({native,selectionToken,active,onActivity,tool}:{native:boolean;selectionToken:string;active:boolean;onActivity:(active:boolean)=>void;tool:BackupToolContext}){
 const [status,setStatus]=useState<Status|null>(null),[error,setError]=useState<ManagerError|null>(null),[pending,setPending]=useState(false);
 const [cancellation,setCancellation]=useState<UpdateCancelReceipt|null>(null),[cancelConsent,setCancelConsent]=useState(false);
 const [references,setReferences]=useState<UpdateReference[]>([]),[referenceId,setReferenceId]=useState('');
 const [application,setApplication]=useState<UpdateApplyReceipt|null>(null),[applyConsent,setApplyConsent]=useState(false),[writersStopped,setWritersStopped]=useState(false);
 const [rollbackContext,setRollbackContext]=useState<UpdateRollbackContext|null>(null),[rollbackReceipt,setRollbackReceipt]=useState<UpdateRollbackReceipt|null>(null);
 const [maintenanceImage,setMaintenanceImage]=useState(''),[keyFile,setKeyFile]=useState(''),[archive,setArchive]=useState(''),[port,setPort]=useState(4500);
 const [preferredRollbackAction,setPreferredRollbackAction]=useState<'restoreOriginal'|'completeRestored'>('completeRestored');
 const [rollbackConsent,setRollbackConsent]=useState(false),[rollbackWritersStopped,setRollbackWritersStopped]=useState(false);
 const live=useRef(true),working=useRef(false),epoch=useRef(0);
 useEffect(()=>{live.current=true;return()=>{live.current=false;epoch.current++;onActivity(false);};},[]);
 async function refresh(){
  const run=++epoch.current;working.current=true;setPending(true);setStatus(null);setError(null);setReferences([]);setReferenceId('');setApplication(null);setCancellation(null);setApplyConsent(false);setWritersStopped(false);setCancelConsent(false);setRollbackContext(null);setRollbackReceipt(null);setRollbackConsent(false);setRollbackWritersStopped(false);setPreferredRollbackAction('completeRestored');
  if(!native){working.current=false;setPending(false);return;}
  try{
   const result=await client.updateStatus(),cancelled=await client.updateCancellation();
   const available=result.stage==='prepared'&&!result.writeUncertain?await client.updateReferences():[];
   if(available.some(reference=>reference.operationId!==result.operationId||reference.generation!==result.generation))throw Error('MANAGER_PROTOCOL');
   const applied=result.authorityPresent?await client.updateApplication():null;
   const canReviewRollback=result.authorityPresent&&result.stage!==null&&result.stage!=='prepared'&&!result.writeUncertain;
   const recoveryContext=canReviewRollback?await client.updateRollbackContext():null;
   if(recoveryContext&&(recoveryContext.installationId!==result.installationId||recoveryContext.operationId!==result.operationId||recoveryContext.generation!==result.generation||recoveryContext.stage!==result.stage))throw Error('MANAGER_PROTOCOL');
   const recovered=canReviewRollback?await client.updateRollback():null;
   if(live.current&&epoch.current===run){setStatus(result);setCancellation(cancelled);setReferences(available);setReferenceId(available.length===1?(available[0]?.referenceId??''):'');setApplication(applied);setRollbackContext(recoveryContext);setRollbackReceipt(recovered);}
  }catch(e){if(live.current&&epoch.current===run){setError(managerError(e));onActivity(false);}}
  finally{if(live.current&&epoch.current===run){working.current=false;setPending(false);}}
 }
 useEffect(()=>{void refresh();},[native,selectionToken]);
 async function cancel(){
  if(!native||active||working.current||!cancelConsent||status?.stage!=='prepared'||!status.operationId||status.generation===null||status.writeUncertain)return;
  const run=epoch.current;working.current=true;onActivity(true);setPending(true);setError(null);
  try{const result=await client.updateCancellation({operationId:status.operationId,generation:status.generation,preserveData:true,closeAppAndCancel:true});if(!result)throw Error('MANAGER_PROTOCOL');if(live.current&&epoch.current===run){setCancellation(result);setCancelConsent(false);if(result.state!=='awaiting-app-close')onActivity(false);}}
  catch(e){if(live.current&&epoch.current===run){setError(managerError(e));onActivity(false);}}
  finally{if(live.current&&epoch.current===run){working.current=false;setPending(false);}}
 }
 const selected=references.find(reference=>reference.referenceId===referenceId);
 async function apply(){
  if(!native||active||working.current||!applyConsent||!writersStopped||status?.stage!=='prepared'||status.writeUncertain||!selected||selected.changedSchema||selected.operationId!==status.operationId||selected.generation!==status.generation)return;
  const run=epoch.current;working.current=true;onActivity(true);setPending(true);setError(null);
  try{const receipt=await client.updateApplication({referenceId:selected.referenceId,operationId:selected.operationId,generation:selected.generation,preserveData:true,closeAppAndApply:true,externalWritersQuiesced:true});if(!receipt)throw Error('MANAGER_PROTOCOL');if(live.current&&epoch.current===run){setApplication(receipt);setApplyConsent(false);setWritersStopped(false);if(receipt.state!=='awaiting-app-close')onActivity(false);}}
  catch(e){if(live.current&&epoch.current===run){setError(managerError(e));onActivity(false);}}
  finally{if(live.current&&epoch.current===run){working.current=false;setPending(false);}}
 }
 const rollbackAction=rollbackContext?.canRestoreOriginal&&(preferredRollbackAction==='restoreOriginal'||!rollbackContext.canCompleteRestored)?'restoreOriginal':rollbackContext?.canCompleteRestored?'completeRestored':null;
 const rollbackMatches=!!status&&!!rollbackContext&&rollbackContext.installationId===status.installationId&&rollbackContext.operationId===status.operationId&&rollbackContext.generation===status.generation&&rollbackContext.stage===status.stage;
 const rollbackFieldsValid=/^sha256:[a-f0-9]{64}$/.test(maintenanceImage)&&(rollbackAction==='completeRestored'||validVerificationInput({image:maintenanceImage,keyPath:keyFile,sourcePath:archive})&&Number.isInteger(port)&&port>=1024&&port<=65535);
 const rollbackAllowed=rollbackMatches&&!status?.writeUncertain&&!!rollbackAction&&rollbackFieldsValid&&rollbackConsent&&rollbackWritersStopped;
 function resetRollbackConsent(){setRollbackConsent(false);setRollbackWritersStopped(false);}
 async function rollback(){
  if(!native||active||working.current||!rollbackAllowed||!rollbackContext||!rollbackAction)return;
  const run=epoch.current;working.current=true;onActivity(true);setPending(true);setError(null);
  try{
   const receipt=await client.updateRollback({action:rollbackAction,operationId:rollbackContext.operationId,generation:rollbackContext.generation,maintenanceImage,keyFile:rollbackAction==='restoreOriginal'?keyFile:null,archive:rollbackAction==='restoreOriginal'?archive:null,port:rollbackAction==='restoreOriginal'?port:null,preserveData:true,closeAppAndRollback:true,externalWritersQuiesced:true});
   if(!receipt)throw Error('MANAGER_PROTOCOL');
   if(live.current&&epoch.current===run){setRollbackReceipt(receipt);resetRollbackConsent();if(receipt.state!=='awaiting-app-close')onActivity(false);}
  }catch(e){if(live.current&&epoch.current===run){setError(managerError(e));onActivity(false);}}
  finally{if(live.current&&epoch.current===run){working.current=false;setPending(false);}}
 }
 const recovery=status?.stage==='recovery_required'||status?.stage==='restoring'||status?.stage==='awaiting_rollback_health';
 return <section className="panel" aria-label="업데이트 상태" data-needs-attention={recovery||status?.stage==='applying'||status?.stage==='awaiting_health'||!!status?.failure||!!status?.writeUncertain||pending||!!application&&application.state!=='updated'||!!rollbackReceipt&&rollbackReceipt.state!=='rolled-back'||!!cancellation&&cancellation.state!=='cancelled'}>
  <h2>업데이트와 복원 상태</h2><p>선택한 관리 공간의 업데이트 기록을 확인합니다. 기록 확인만으로 전시를 변경하지 않습니다.</p>
  <button disabled={!native||active||pending} onClick={()=>void refresh()}>{pending?'업데이트 요청 처리 중…':'업데이트 상태 확인'}</button>
  <div role="status" aria-live="polite">{pending&&<p>작업을 처리하고 있습니다. 앱이 종료되면 다음 실행에서 결과를 확인하세요.</p>}{status&&<>
   <h3>{status.stage?labels[status.stage]:status.authorityPresent?'업데이트 작업 기록 없음':'서명된 업데이트 권한이 준비되지 않음'}</h3>
   {status.operationId&&<p>작업 ID: {status.operationId}</p>}{status.generation!==null&&<p>기록 번호: {status.generation}</p>}
   <p>저장된 완료 기록은 현재 서버의 정상 응답을 대신하지 않습니다. 상단의 서버 준비 상태를 함께 확인하세요.</p>
   {recovery&&<p>기존 전시·백업·키·복원 후보를 보존하세요. 원래 전체 백업 복원과 응답 확인이 필요합니다.</p>}
   {status.failure&&<p>완료하지 못한 작업: {({update_failed:'업데이트 적용',health_failed:'전시 응답 확인',restore_failed:'원래 전시 복원',interrupted:'작업 중단'})[status.failure]}. 자동 재실행하지 말고 보존한 기록을 확인하세요.</p>}
   {status.writeUncertain&&<p>기록 완료 여부를 확인할 수 없습니다. 이전 기록과 후보를 보존하세요.</p>}
  </>}</div>
  {error&&<div role="alert"><p>업데이트 요청을 완료하지 못했습니다. 확인 코드: {error.code}</p><p>원래 데이터·권한·후보를 유지하고 다른 작업이 종료된 뒤 상태를 확인하세요.</p></div>}
  {status?.stage==='prepared'&&!status.writeUncertain&&<>
   <fieldset disabled={!native||active||pending}><legend>준비된 업데이트 적용</legend>
    {references.length===0?<p>등록된 실행 자료가 없습니다. 준비와 복구 검증을 마친 뒤 다시 확인하세요.</p>:<>
     <label>준비된 버전<select value={referenceId} onChange={event=>{setReferenceId(event.target.value);setApplyConsent(false);setWritersStopped(false);}}><option value="">선택하세요</option>{references.map(reference=><option key={reference.referenceId} value={reference.referenceId}>{reference.version} · {reference.referenceId.slice(0,8)}</option>)}</select></label>
     {selected&&<p>{selected.changedSchema?'데이터 구조가 바뀌는 업데이트의 앱 적용은 아직 검증되지 않았습니다. 이 참조는 적용할 수 없습니다.':'적용 전에 서명·원본·전체 복구 자료를 다시 검사합니다. 데이터 구조는 유지합니다.'}</p>}
     <label><input type="checkbox" checked={writersStopped} onChange={event=>setWritersStopped(event.target.checked)}/>외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.</label>
     <label><input type="checkbox" checked={applyConsent} onChange={event=>setApplyConsent(event.target.checked)}/>기존 데이터·백업·키를 보존하고 앱을 닫아 업데이트를 적용합니다. 적용 중 전시가 중단됩니다.</label>
     <button disabled={!selected||selected.changedSchema||!writersStopped||!applyConsent} onClick={()=>void apply()}>앱을 닫고 업데이트 적용</button>
     <p>앱이 완전히 종료된 뒤 실행합니다. 다시 앱을 열어 실제 결과를 확인하세요.</p>
    </>}
   </fieldset>
   <details><summary>아직 실행하지 않은 업데이트 취소</summary><fieldset disabled={!native||active||pending}>
    <label><input type="checkbox" checked={cancelConsent} onChange={event=>setCancelConsent(event.target.checked)}/>앱을 닫고 준비 기록만 취소합니다. 원래 전시·백업·키·후보·이력을 모두 보존합니다.</label>
    <button disabled={!cancelConsent} onClick={()=>void cancel()}>앱을 닫고 준비된 업데이트 취소</button>
   </fieldset></details>
  </>}
  {rollbackContext&&<div className="backup-verification backup-creation"><fieldset disabled={!native||active||pending||!rollbackMatches||!!status?.writeUncertain}>
   <legend>원래 전시 전체 복원</legend>
   <p>업데이트 전 전체 백업으로 별도 후보를 복원합니다. 현재 전시·실패 후보·백업·키를 덮어쓰거나 삭제하지 않습니다.</p>
   {rollbackContext.selectionReconciliationRequired&&<p role="alert">복원된 후보와 관리 공간의 선택 기록을 다시 맞춰야 합니다. 기존 자료를 보존하며, 아래 동의 후 앱이 선택 기록 확인·복구와 실제 응답 확인을 이어갑니다.</p>}
   <details><summary>복원 대상과 등록된 자료</summary><dl>
    <dt>원래 백업 ID</dt><dd>{rollbackContext.backupId}</dd><dt>원래 이미지</dt><dd><code>{rollbackContext.sourceImage}</code></dd>
    <dt>원래 데이터 구조</dt><dd><code>{rollbackContext.sourceSchema}</code></dd><dt>백업 manifest</dt><dd><code>{rollbackContext.backupManifest}</code></dd><dt>원본 목록</dt><dd><code>{rollbackContext.sourceInventory}</code></dd>
   </dl><p>자료 등록은 복원·활성화 완료를 뜻하지 않습니다.</p></details>
   {rollbackContext.canRestoreOriginal&&rollbackContext.canCompleteRestored&&<details><summary>복원 방법 선택</summary><label>진행할 복원 방법<select value={rollbackAction??''} onChange={e=>{setPreferredRollbackAction(e.target.value as 'restoreOriginal'|'completeRestored');resetRollbackConsent();}}><option value="completeRestored">복원된 후보 확인 계속</option><option value="restoreOriginal">원래 전체 백업을 새 후보에 다시 복원</option></select></label><p>새 후보에 다시 복원해도 기존 복원 후보와 원본 자료는 보존합니다.</p></details>}
   {rollbackAction?<>
    <p>{rollbackAction==='completeRestored'?'전체 복원 후보가 준비됐습니다. 원본 자료를 다시 풀지 않고 후보 활성화와 전시 응답 확인을 이어갑니다.':'전체 자료를 별도 후보에 복원한 뒤 활성화와 전시 응답을 확인합니다. 이미지 파일만 되돌려 완료 처리하지 않습니다.'}</p>
    <BackupToolField id="rollback-image" label="복원용 유지보수 이미지의 고정 ID" value={maintenanceImage} onChange={value=>{setMaintenanceImage(value);resetRollbackConsent();}} context={tool}/>
    {rollbackAction==='restoreOriginal'&&<>
     <BackupPathField id="rollback-archive" label="원래 전체 백업 폴더의 전체 경로" kind="archive" value={archive} onChange={value=>{setArchive(value);resetRollbackConsent();}} disabled={!native||active||pending||!rollbackMatches||!!status?.writeUncertain}/>
     <BackupPathField id="rollback-key" label="복원에 사용할 별도 키 파일의 전체 경로" kind="key" value={keyFile} onChange={value=>{setKeyFile(value);resetRollbackConsent();}} disabled={!native||active||pending||!rollbackMatches||!!status?.writeUncertain}/>
     <p>키 내용은 입력하지 마세요. 경로 입력은 이 화면에서만 유지합니다.</p>
     <label>복원 후보의 로컬 포트<input type="number" min={1024} max={65535} step={1} value={Number.isFinite(port)?port:''} onChange={e=>{setPort(e.target.value===''?NaN:Number(e.target.value));resetRollbackConsent();}}/></label>
    </>}
    <label className="backup-ack"><input type="checkbox" checked={rollbackWritersStopped} onChange={e=>setRollbackWritersStopped(e.target.checked)}/>외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.</label>
    <label className="backup-ack"><input type="checkbox" checked={rollbackConsent} onChange={e=>setRollbackConsent(e.target.checked)}/>전시 중단을 이해하며, 현재 전시·후보·백업·키를 보존하고 앱을 닫아 원래 전시 전체 복원을 진행합니다.</label>
    <button className="primary" disabled={!rollbackAllowed} onClick={()=>void rollback()}>{rollbackAction==='completeRestored'?'앱을 닫고 복원된 전시 확인':'앱을 닫고 원래 전시 전체 복원'}</button>
    <p>앱이 완전히 종료된 뒤 실행합니다. 다시 열어 복원·활성화·실제 응답 확인 결과를 확인하세요.</p>
   </>:<p>현재 기록에서는 전체 복원을 실행할 수 없습니다. 후보를 보존하고 진단 결과를 확인하세요.</p>}
  </fieldset></div>}
  {rollbackReceipt&&<p role="status">전체 복원 요청: {({'awaiting-app-close':'앱 종료 또는 실행 대기',uncertain:'완료 여부 확인 불가 — 기록과 후보를 보존하세요',failed:'전체 복원 실패', 'rolled-back':'원래 전시 전체 복원 완료 기록'})[rollbackReceipt.state]}{rollbackReceipt.code&&` · ${rollbackReceipt.code}`}</p>}
  {application&&<p role="status">적용 요청: {({'awaiting-app-close':'앱 종료 또는 실행 대기',uncertain:'완료 여부 확인 불가 — 기록과 데이터를 보존하세요',failed:'적용 실패',updated:'업데이트 적용 완료 기록'})[application.state]}{application.code&&` · ${application.code}`}</p>}
  {cancellation&&<p role="status">취소 요청: {({'awaiting-app-close':'앱 종료 또는 실행 대기',uncertain:'완료 여부 확인 불가 — 기록과 데이터를 보존하세요',failed:'취소 실패',cancelled:'준비된 업데이트 취소 완료'})[cancellation.state]}{cancellation.code&&` · ${cancellation.code}`}</p>}
  <p className="note">실행 중인 업데이트는 준비 기록 취소로 중단할 수 없습니다. 실패·중단 후에는 현재 기록에서 허용된 복원 경로를 사용하고, 완료 여부가 불확실하면 후보를 보존하세요.</p>
 </section>;
}
