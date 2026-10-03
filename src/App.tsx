// SPDX-License-Identifier: Apache-2.0
import {useCallback,useEffect,useRef,useState} from 'react';
import {client,managerError,statusLabel,actionLabel,formatBytes,serviceStateLabel,healthLabel,nextStep,type Status,type EngineProbe,type Job,type LogEvent,type ManagerError,type Action} from './lifecycle';
import './style.css';
import {BackupVerification} from './BackupVerification';
import {BackupCreation} from './BackupCreation';
import {BackupRestoration} from './BackupRestoration';
import {HelperReconciliation} from './HelperReconciliation';
import {InstallationSelection} from './InstallationSelection';
import {MaintenanceCancellation} from './MaintenanceCancellation';
import type {VerificationInput,CreationInput,BackupJob,RestorationInput,RestorationContext,InstallationContext,ReconciliationInput,ReconciliationJob} from './lifecycle';
export function App(){
 const [cancellationActive,setCancellationActive]=useState(false);
 const [status,setStatus]=useState<Status|null>(null),[engines,setEngines]=useState<EngineProbe[]>([]),[jobs,setJobs]=useState<Job[]>([]),[logs,setLogs]=useState<LogEvent[]>([]),[error,setError]=useState<ManagerError|null>(null),[busy,setBusy]=useState(false),[loading,setLoading]=useState(false),[notice,setNotice]=useState('');
 const [backupJobs,setBackupJobs]=useState<BackupJob[]>([]),[backupHistoryError,setBackupHistoryError]=useState<ManagerError|null>(null);
 const [restoration,setRestoration]=useState<RestorationContext|null>(null),[restorationError,setRestorationError]=useState<ManagerError|null>(null);
 const [installation,setInstallation]=useState<InstallationContext|null>(null),[installationError,setInstallationError]=useState<ManagerError|null>(null);
 const [reconciliationJobs,setReconciliationJobs]=useState<ReconciliationJob[]>([]),[reconciliationError,setReconciliationError]=useState<ManagerError|null>(null);
 const live=useRef(true),working=useRef(false),refreshing=useRef(false),statusGeneration=useRef(0);
 const refresh=useCallback(async()=>{
  if(!client.native||refreshing.current)return;
  const generation=statusGeneration.current;
  refreshing.current=true;if(live.current)setLoading(true);
  try{
   try{const selected=await client.installations();if(live.current&&generation===statusGeneration.current){setInstallation(selected);setInstallationError(null);}}
   catch(e){if(live.current&&generation===statusGeneration.current){setInstallation(null);setInstallationError(managerError(e));}throw e;}
   const result=await Promise.all([client.status(),client.engines(),client.jobs(),client.logs()]);
   if(live.current&&generation===statusGeneration.current){setStatus(result[0]);setEngines(result[1]);setJobs(result[2]);setLogs(result[3]);}
   try{const saved=await client.backupJobs();if(live.current&&generation===statusGeneration.current){setBackupJobs(saved);setBackupHistoryError(null);}}
   catch(e){if(live.current&&generation===statusGeneration.current)setBackupHistoryError(managerError(e));}
  }catch(e){if(live.current&&generation===statusGeneration.current){setStatus(null);setError(managerError(e));}}
  finally{
   try{const saved=await client.restorationContext();if(live.current&&generation===statusGeneration.current){setRestoration(saved);setRestorationError(null);}}
   catch(e){if(live.current&&generation===statusGeneration.current){setRestoration(null);setRestorationError(managerError(e));}}
   try{const saved=await client.helperReconciliations();if(live.current&&generation===statusGeneration.current){setReconciliationJobs(saved);setReconciliationError(null);}}
   catch(e){if(live.current&&generation===statusGeneration.current)setReconciliationError(managerError(e));}
   refreshing.current=false;if(live.current)setLoading(false);
  }
 },[]);
 useEffect(()=>{live.current=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{if(!working.current)await refresh();if(live.current)timer=setTimeout(()=>void poll(),3000);};void poll();return()=>{live.current=false;clearTimeout(timer);};},[refresh]);
 async function operation(action:'install'|Action){if(working.current||!client.native)return;working.current=true;setBusy(true);setError(null);setNotice(`${actionLabel(action)} 작업을 진행합니다. 창을 닫아도 저장된 작업 기록은 남습니다.`);try{const result=action==='install'?await client.install():await client.action(action);if(!live.current)return;if(result.state==='failed'||result.state==='interrupted')setError({code:result.errorCode??'JOB_FAILED',guidance:result.guidance??'작업 기록을 확인하고 다시 시도하세요.'});else setNotice(result.state==='completed'?`${actionLabel(action)} 작업이 완료됐습니다.`:'진행 중인 작업 기록을 확인합니다.');await refresh();}catch(e){if(live.current)setError(managerError(e));}finally{working.current=false;if(live.current)setBusy(false);}}
 async function verifyBackup(input:VerificationInput){
  if(working.current||!client.native)throw {code:'BUSY',guidance:'진행 중인 작업이 끝날 때까지 기다려 주세요.'};
  working.current=true;setBusy(true);setError(null);setNotice('백업 사본의 인증을 확인하고 있습니다.');
  try{const result=await client.verifyBackup(input);if(live.current)setNotice('백업 사본 인증을 통과했습니다. 데이터 복원은 실행하지 않았습니다.');return result;}
  catch(e){if(live.current)setNotice('백업 사본 인증을 통과하지 못했습니다. 검증 안내를 확인하세요.');throw e;}
  finally{working.current=false;if(live.current){setBusy(false);void refresh();}}
 }
 async function createBackup(input:CreationInput){
  if(working.current||!client.native)throw {code:'BUSY',guidance:'진행 중인 작업이 끝날 때까지 기다려 주세요.'};
  statusGeneration.current++;working.current=true;setBusy(true);setError(null);setStatus(null);setNotice('전시를 멈추고 암호화 백업을 생성합니다.');
  try{const result=await client.createBackup(input);if(live.current)setNotice('암호화 백업 생성과 인증을 완료했습니다. 전시 상태를 확인하고 직접 다시 시작하세요.');return result;}
  catch(e){if(live.current)setNotice('백업 완료 여부를 확인하지 못했습니다. 백업 기록과 남은 유지보수 작업을 확인하세요.');throw e;}
  finally{working.current=false;if(live.current){setBusy(false);void refresh();}}
 }
 async function restoreBackup(input:RestorationInput){
  if(working.current||!client.native)throw {code:'BUSY',guidance:'진행 중인 작업이 끝날 때까지 기다려 주세요.'};
  statusGeneration.current++;working.current=true;setBusy(true);setError(null);setStatus(null);setRestoration(null);setNotice('암호화 사본을 새 설치로 복원하고 전시 응답을 확인합니다.');
  try{const result=await client.restoreBackup(input);if(live.current)setNotice('새 전시의 복원과 시작을 확인했습니다. 실제 상태를 다시 확인합니다.');return result;}
  catch(e){if(live.current)setNotice('복원 완료 여부를 확인하지 못했습니다. 저장된 복원 기록을 확인하세요.');throw e;}
  finally{working.current=false;if(live.current){setBusy(false);void refresh();}}
 }
 async function reconcileHelper(input:ReconciliationInput){
  if(working.current||!client.native)throw {code:'BUSY',guidance:'진행 중인 작업이 끝날 때까지 기다려 주세요.'};
  statusGeneration.current++;working.current=true;setBusy(true);setStatus(null);setError(null);setNotice('실패·중단 작업의 helper 소유권을 확인하고 정지 여부를 검사합니다.');
  try{const result=await client.reconcileHelper(input);if(live.current)setNotice('해당 helper의 정지 또는 부재를 확인했습니다. 원래 작업은 실패·중단 상태이며 서버를 재개하거나 데이터를 삭제하지 않았습니다.');return result;}
  catch(e){if(live.current)setNotice('helper 정지 여부가 확인되지 않았습니다. 후보와 원래 작업 기록을 보존하고 진단하세요.');throw e;}
  finally{working.current=false;if(live.current){setBusy(false);void refresh();}}
 }
 async function changeInstallation(target:string|null,preserveExisting:boolean){
  if(working.current||!client.native)throw {code:'BUSY',guidance:'진행 중인 작업이 끝날 때까지 기다려 주세요.'};
  statusGeneration.current++;working.current=true;setBusy(true);setStatus(null);setRestoration(null);setInstallation(null);setError(null);setJobs([]);setLogs([]);setBackupJobs([]);setReconciliationJobs([]);setReconciliationError(null);setBackupHistoryError(null);setRestorationError(null);setNotice('기존 데이터를 보존하고 관리 공간을 변경합니다.');
  try{const result=target===null?await client.createInstallation(preserveExisting):await client.selectInstallation(target,preserveExisting);if(live.current){setInstallation(result);setInstallationError(null);setNotice('관리 공간 선택을 저장했습니다. 실제 상태를 다시 확인합니다. 서버를 자동 정지하거나 데이터를 이동하지 않았습니다.');}return result;}
  catch(e){if(live.current)setNotice('관리 공간 변경 여부가 확인되지 않았습니다. 기존 폴더와 선택 기록을 보존하고 상태를 다시 확인하세요.');throw e;}
  finally{working.current=false;if(live.current){setBusy(false);void refresh();}}
 }
 async function open(){setError(null);try{await client.open();setNotice('기본 브라우저에서 내 컴퓨터의 전시를 열었습니다.');}catch(e){setError(managerError(e));}}
 const latestJob=jobs.at(-1),currentJob=status?.activeJob??(latestJob?.state==='running'?latestJob:null),recoverable=latestJob&&['failed','interrupted'].includes(latestJob.state)?latestJob:null;
 const installationBlocked=!installation||!!installationError||!!installation.errorCode||!installation.installations.find(root=>root.id===installation.activeId)?.available;
 const restorationBlocked=!!restorationError||!restoration||!!restoration.job&&restoration.job.state!=='completed';
 const active=cancellationActive||busy||!!status?.activeJob||backupJobs.some(job=>job.state==='running')||restoration?.job?.state==='running'||reconciliationJobs.some(job=>job.state==='checking'),engineReady=engines.some(e=>e.available),canStart=!!status?.installed&&engineReady&&!active&&!backupHistoryError&&!reconciliationError&&!restorationBlocked&&!installationBlocked,ready=status?.readiness.ready===true&&!restorationBlocked&&!installationBlocked;
 return <main><header><a className="skip" href="#controls">전시 관리로 건너뛰기</a><div className="brand">E<span>ExhibitOS<br/><small>MANAGER</small></span></div><p className="local-tag">내 컴퓨터에서 관리</p></header>
 <section className="welcome"><p className="eyebrow">LOCAL EXHIBITION</p><h1>전시를 시작할 준비</h1><p>실행 도구를 확인하고 전시를 설치하세요. 작품과 전시 데이터는 정지해도 보존됩니다.</p></section>
 {!client.native&&<div className="alert" role="status"><strong>데스크톱 앱에서 열어주세요.</strong><p>웹 브라우저에서는 컴퓨터의 실행 도구와 실제 전시 상태를 조회하거나 변경할 수 없습니다.</p></div>}
 {error&&<div className="alert" role="alert"><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><button disabled={active||loading} onClick={()=>{setError(null);void refresh();}}>상태 다시 확인</button></div>}
 <aside className="next-step" aria-label="다음 할 일"><strong>다음 할 일</strong><p>{installationBlocked&&client.native&&!active?'현재 관리 공간을 확인하거나 아래에서 새 복원 공간을 선택하세요. 기존 데이터는 지우지 않습니다.':restorationBlocked&&client.native&&!active?'저장된 복원 상태를 확인하세요. 실패·중단 후보는 보존하며 덮어쓰거나 일반 다시 시도를 실행하지 않습니다.':backupHistoryError&&!active?'저장된 백업 기록을 확인할 수 없어 변경을 멈췄습니다. 실행 도구 다시 확인을 누르세요.':recoverable&&!active?'아래 작업 진행과 복구 안내를 확인한 뒤 실패한 작업 다시 시도를 누르세요.':nextStep(status,engineReady,active,client.native)}</p></aside>
 <InstallationSelection native={client.native} active={active} loading={loading} context={installation} historyError={installationError} change={changeInstallation}/>
 <div className="dashboard"><section className="panel status-panel" aria-label="실제 전시 상태"><div className="panel-top"><h2>전시 상태</h2><span className={`status-dot ${ready?'ready':''}`}>{status?installationBlocked?'관리 공간 확인 필요':restorationBlocked&&status.installed?'복원 상태 확인 필요':statusLabel(status.state):'확인되지 않음'}</span></div><dl><div><dt>설치</dt><dd>{status?status.installed?'설치됨':'설치 전':'확인되지 않음'}</dd></div><div><dt>서버 준비</dt><dd>{status?ready?'준비 완료':status.installed?'아직 준비되지 않음':'설치 후 확인':'확인되지 않음'}</dd></div><div><dt>실행 패키지</dt><dd>{status?.version??'확인되지 않음'}</dd></div><div><dt>서버 버전</dt><dd>{status?.readiness.version??'확인되지 않음'}</dd></div></dl>{status?.services.length? <ul className="service-list">{status.services.map(s=><li key={s.name}><span>{s.name}</span><span>{serviceStateLabel(s.state)} · {healthLabel(s.health)}</span></li>)}</ul>:<p className="note">서비스 실행 상태는 설치한 후 확인할 수 있습니다.</p>}<button className="primary" disabled={!client.native||!ready||active} onClick={()=>void open()}>전시 열기</button></section>
 <section className="panel" id="controls" tabIndex={-1} aria-label="전시 설치와 실행"><h2>설치와 실행</h2><ol className="steps"><li><span>1</span><div><h3>실행 도구 확인</h3><p>Docker 또는 Podman과 Compose가 실행돼 있어야 합니다.</p>{engines.length?<ul className="engine-list">{engines.map(e=><li key={e.kind}><strong>{e.kind==='docker'?'Docker':e.kind==='podman'?'Podman':e.kind}</strong> {e.available?'사용 가능':e.installed?'시작 또는 설정 필요':'설치 필요'}{e.engineVersion&&<small> 버전 {e.engineVersion}</small>}{!e.available&&e.guidance&&<p>{e.guidance}</p>}</li>)}</ul>:<p className="note">{loading?'실행 도구를 확인하고 있습니다.':'실행 도구를 아직 확인하지 못했습니다.'}</p>}<button disabled={!client.native||loading||active} onClick={()=>void refresh()}>실행 도구 다시 확인</button></div></li><li><span>2</span><div><h3>검증된 전시 설치</h3><p>준비된 설치 패키지를 검증한 후 설치합니다. 설치 후에 전시를 시작할 수 있습니다.</p><button disabled={!client.native||!engineReady||active||!!backupHistoryError||!!reconciliationError||restorationBlocked||installationBlocked||!status||!!status.installed} onClick={()=>void operation('install')}>전시 설치</button></div></li><li><span>3</span><div><h3>전시 시작</h3><p>시작하면 서버가 준비됐는지 확인합니다. 정지는 저장 데이터를 삭제하지 않습니다.</p><div className="button-row"><button className="primary" disabled={!canStart||ready} onClick={()=>void operation('start')}>시작</button><button disabled={!client.native||!status?.installed||!engineReady||active||status?.state==='stopped'} onClick={()=>void operation('stop')}>정지</button><button disabled={!canStart} onClick={()=>void operation('restart')}>재시작</button></div></div></li></ol></section></div>
 <section className="panel storage" aria-label="저장 공간"><div><h2>저장 공간</h2><p>전시 데이터는 별도로 보존됩니다. 사용량이 측정되지 않으면 아래에 표시합니다.</p></div><dl><div><dt>전시 사용량</dt><dd>{formatBytes(status?.storage.usedBytes??null)}</dd></div><div><dt>디스크 여유 공간</dt><dd>{status?formatBytes(status.storage.freeBytes):'확인되지 않음'}</dd></div><div><dt>필요한 여유 공간</dt><dd>{status?formatBytes(status.storage.minimumFreeBytes):'확인되지 않음'}</dd></div>{status?.storage.quotaBytes!==undefined&&<div><dt>안내 용량</dt><dd>{formatBytes(status.storage.quotaBytes)}</dd></div>}</dl></section>
 <MaintenanceCancellation key={'cancel:'+(installation?.selectionToken??'unknown-installation')} native={client.native} available={!installationBlocked} onActivity={setCancellationActive}/>
 <HelperReconciliation key={'helper:'+(installation?.selectionToken??'unknown-installation')} native={client.native} active={active||installationBlocked} backupJobs={backupJobs} restoration={restoration} targetError={backupHistoryError??restorationError} history={reconciliationJobs} historyError={reconciliationError} reconcile={reconcileHelper}/>
 <BackupRestoration key={'restoration:'+(installation?.selectionToken??'unknown-installation')} native={client.native} active={active||!!backupHistoryError||!!reconciliationError||installationBlocked} engineReady={engines.some(e=>e.kind==='docker'&&e.available)} context={restoration} historyError={restorationError} restore={restoreBackup}/>
 <BackupCreation key={'creation:'+(installation?.selectionToken??'unknown-installation')} native={client.native} installed={status?.installed===true} active={active||!!reconciliationError||restorationBlocked||installationBlocked} engineReady={engineReady} jobs={backupJobs} historyError={backupHistoryError} create={createBackup}/>
 <BackupVerification key={'verification:'+(installation?.selectionToken??'unknown-installation')} native={client.native} installed={status?.installed===true} active={active||!!backupHistoryError||!!reconciliationError||restorationBlocked||installationBlocked} engineReady={engineReady} verify={verifyBackup}/>
 <section className="panel" aria-label="저장된 작업 기록"><div className="panel-top"><h2>작업 기록</h2><button disabled={!client.native||active||!!backupHistoryError||!!reconciliationError||restorationBlocked||installationBlocked||!status||!recoverable} onClick={()=>void operation('retry')}>실패한 작업 다시 시도</button></div>{(active||recoverable)&&<div className={`job-summary ${recoverable&&!active?'needs-attention':''}`} role="region" aria-label="작업 진행과 복구"><h3>{active?`${currentJob?actionLabel(currentJob.action):'요청한'} 작업 진행 중`:'다시 시도하기 전에 확인하세요'}</h3>{active?<><p>실행 도구의 응답을 기다리고 있습니다. 완료까지 걸리는 시간은 환경에 따라 다릅니다.</p>{currentJob&&<progress aria-label="현재 작업 진행률" max="100" value={currentJob.progress}/>}</>:<><p>{recoverable?.guidance??'안전한 진단 기록과 실행 도구 상태를 확인한 뒤 다시 시도하세요.'}</p><p>문제를 해결한 뒤 실패한 작업 다시 시도를 누르세요. 저장 데이터는 자동 삭제하지 않습니다.</p></>}</div>}<p role="status" aria-live="polite">{notice||'설치·시작·정지 작업의 결과를 여기에 저장합니다.'}</p>{jobs.length?<ol className="job-list">{[...jobs].reverse().slice(0,10).map(j=><li key={`${j.id}:${j.attempt}`}><strong>{actionLabel(j.action)}</strong><span>{j.state==='completed'?'완료':j.state==='failed'?'실패':j.state==='interrupted'?'중단':'진행 중'}</span><time>{new Date(j.updatedAt).toLocaleString('ko-KR')}</time>{j.state==='running'&&<progress aria-label={`${actionLabel(j.action)} 진행률`} max="100" value={j.progress}/ >}{j.guidance&&<p>{j.guidance}</p>}</li>)}</ol>:<p className="note">아직 실행한 작업이 없습니다. 위의 실행 도구 확인부터 시작하세요.</p>}<details><summary>안전한 진단 기록</summary>{logs.length?<ul>{logs.slice(-20).map((l,i)=><li key={`${l.at}-${i}`}><time>{new Date(l.at).toLocaleString('ko-KR')}</time> {l.message}</li>)}</ul>:<p>진단 기록이 없습니다. 비밀번호와 컨테이너 원본 로그는 이 화면에 표시하지 않습니다.</p>}</details></section>
 <footer>ExhibitOS Manager · 로컬 전시 관리 <span>{loading?'상태 확인 중…':'실제 실행 상태와 저장된 작업 기록을 확인합니다.'}</span></footer></main>;
}
