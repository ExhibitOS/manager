// SPDX-License-Identifier: Apache-2.0
import {type ExternalInput,type ExternalContext} from './external-restoration';
import {BackupPathField} from './BackupPathField';
import {BackupToolField,type BackupToolContext} from './BackupToolField';
import {useEffect,useRef,useState} from 'react';
import {managerError,validVerificationInput,type ManagerError,type VerificationInput,type VerificationReceipt} from './lifecycle';
interface Props{historyVisible:boolean;savedBackup?:{path:string;keyPath:string}|null;tool:BackupToolContext;native:boolean;installed:boolean;preinstallReady?:boolean;active:boolean;engineReady:boolean;verify(input:VerificationInput):Promise<VerificationReceipt>;externalRestore(input:ExternalInput):Promise<ExternalContext>}
export function BackupVerification({historyVisible,savedBackup,tool,native,installed,preinstallReady=false,active,engineReady,verify,externalRestore}:Props){
 const [input,setInput]=useState<VerificationInput>({image:'',keyPath:'',sourcePath:''});
 const [result,setResult]=useState<VerificationReceipt|null>(null),[error,setError]=useState<ManagerError|null>(null),[pending,setPending]=useState(false);
 const [external,setExternal]=useState<ExternalContext>({sources:[],checks:[]}),[sourceId,setSourceId]=useState(''),[externalPending,setExternalPending]=useState(false),[externalError,setExternalError]=useState<ManagerError|null>(null),[externalNotice,setExternalNotice]=useState(''),[externalLoading,setExternalLoading]=useState(false);
 const externalEpoch=useRef(0),contextRequest=useRef(false),externalWorking=useRef(false);
 const errorElement=useRef<HTMLDivElement>(null),working=useRef(false),live=useRef(true);
 useEffect(()=>{live.current=true;return()=>{live.current=false;externalEpoch.current++;};},[]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 useEffect(()=>{if(savedBackup){setInput(previous=>({...previous,sourcePath:savedBackup.path,...(savedBackup.keyPath?{keyPath:savedBackup.keyPath}:{})}));setResult(null);setError(null);}},[savedBackup]);
 async function refreshExternal(clearError=false){
  if(!native||contextRequest.current)return;
  contextRequest.current=true;setExternalLoading(true);const generation=externalEpoch.current;
  try{const value=await externalRestore({action:'context'});if(live.current&&generation===externalEpoch.current){setExternal(value);if(clearError)setExternalError(null);}}
  catch(e){if(live.current&&generation===externalEpoch.current&&managerError(e).code!=='BUSY')setExternalError(managerError(e));}
  finally{contextRequest.current=false;if(live.current)setExternalLoading(false);}
 }
 useEffect(()=>{if(native&&historyVisible)void refreshExternal();},[native,historyVisible]);
 useEffect(()=>{if(!native||!historyVisible||!external.checks.some(j=>j.state==='running'))return;const timer=setInterval(()=>void refreshExternal(),2000);return()=>clearInterval(timer);},[native,historyVisible,external.checks]);
 const selected=external.sources.find(s=>s.verificationId===sourceId);
 const externalBusy=externalPending||externalWorking.current;
 async function externalAction(request:ExternalInput){
  if(!native||active||externalBusy||working.current)return;
  externalWorking.current=true;const generation=++externalEpoch.current;setExternalPending(true);setExternalError(null);setExternalNotice('');
  try{const value=await externalRestore(request);if(live.current&&generation===externalEpoch.current){setExternal(value);setExternalNotice(request.action==='reconcile'?'남은 검사 도구의 상태를 확인했습니다. 새 검사를 시작할 때 실제 상태를 다시 확인합니다.':'격리 DB 검사 기록을 갱신했습니다. 전체 서비스 복원은 실행하지 않았습니다.');}}
  catch(e){if(live.current&&generation===externalEpoch.current)setExternalError(managerError(e));}
  finally{externalWorking.current=false;if(live.current){setExternalPending(false);void refreshExternal();}}
 }
 const verificationInput=installed?input:{...input,image:tool.image??''};
 const unavailable=!native||(installed?!engineReady:!preinstallReady||!tool.image),disabled=unavailable||active||pending||externalPending;
 function change(name:keyof VerificationInput,value:string){externalEpoch.current++;if(name!=='keyPath')setSourceId('');setExternalNotice('');setExternalError(null);setInput(previous=>({...previous,[name]:value}));setResult(null);setError(null);}
 async function submit(event:React.SubmitEvent<HTMLFormElement>){
  event.preventDefault();if(working.current||disabled||!validVerificationInput(verificationInput))return;
  working.current=true;setPending(true);setError(null);setResult(null);
  try{const receipt=await verify(verificationInput);if(live.current){setResult(receipt);setSourceId(receipt.id);void refreshExternal();}}catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current)setPending(false);}
 }
 return <section className="panel backup-verification" aria-labelledby="backup-heading">
  <p className="eyebrow">PRESERVE & VERIFY</p><h2 id="backup-heading">백업 사본 검증</h2>
  <p>암호화된 백업이 손상되지 않았는지 확인합니다. 원본과 키는 읽기만 하며, 새 비공개 작업 공간에 복호화된 파일이 남습니다.</p>
  <p className="note" id="backup-scope">사본 인증은 DB 복원·서비스 활성화를 수행하지 않습니다. 아래 격리 DB 검사는 별도 후보에만 복원하며 전체 서비스를 복원하지 않습니다.</p>
  {unavailable&&<p className="note">{!native?'데스크톱 앱에서만 검증할 수 있습니다.':!installed?(!preinstallReady?'설치 전 백업 검증에는 Docker 실행 상태 확인이 필요합니다. 실행 도구를 확인한 뒤 다시 시도하세요.':tool.message??'복구 도구가 포함된 Manager에서 도구 정보를 다시 확인하세요.'):'설치에 사용한 실행 도구를 시작하고 상태를 확인하세요.'}</p>}
  {!installed&&<p className="note">전시를 설치하지 않고 보관한 백업과 키를 확인합니다. 검증 파일은 관리 공간에 별도로 보관하며, 새 전시를 설치하거나 DB를 복원하지 않습니다.</p>}
  <form onSubmit={event=>void submit(event)} aria-describedby="backup-scope">
   <fieldset disabled={disabled}><legend>준비한 백업과 외부 키</legend>
    <BackupPathField id="backup-source" label="암호화된 백업 폴더의 전체 경로" kind="archive" value={input.sourcePath} onChange={value=>change('sourcePath',value)} disabled={disabled} helpId="backup-path-help"/>
    <BackupPathField id="backup-key" label="별도로 보관한 키 파일의 전체 경로" kind="key" value={input.keyPath} onChange={value=>change('keyPath',value)} disabled={disabled} helpId="backup-key-help"/>
    <p className="note" id="backup-key-help">키 내용은 입력하지 마세요. 백업 폴더 밖의 비공개 키 파일을 사용합니다.</p>
    {savedBackup&&input.sourcePath===savedBackup.path&&<p className="note" role="status">방금 외부에 보관한 사본을 선택했습니다. 키를 확인한 뒤 검증하세요. 복원은 실행하지 않습니다.</p>}
    <p className="note" id="backup-path-help">파일 선택 창에서 암호화 백업 폴더를 선택하세요. 필요한 경우 경로 직접 입력을 사용할 수 있습니다.</p>
    {installed?<BackupToolField id="backup-image" label="유지보수 이미지의 고정 ID" value={input.image} onChange={value=>change('image',value)} context={tool}/>:<div><p className="note">{tool.image?'이 Manager에 포함된 복구 도구로 백업을 확인합니다.':tool.message??'포함된 복구 도구 정보를 확인하고 있어요.'}</p>{tool.message&&tool.retry&&<button type="button" onClick={tool.retry}>복구 도구 다시 확인</button>}</div>}
   </fieldset>
   <button className="primary" type="submit" disabled={disabled||!validVerificationInput(verificationInput)}>{pending?'백업 인증 확인 중…':'백업 사본 검증'}</button>
  </form>
  {pending&&<div className="job-summary" role="status"><strong>백업 도구를 준비하고 암호화 사본을 확인하고 있습니다.</strong><p>검사가 끝날 때까지 창을 유지하세요. 설치·시작 등 다른 작업은 함께 실행하지 않습니다.</p><progress aria-label="백업 인증 검사 진행 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>입력과 실행 도구 상태를 확인한 뒤 다시 검증하세요. 기존 사본과 키는 보존되며, 실패한 새 작업 공간에는 부분 복호화 파일이 남을 수 있습니다.</p></div>}
  {result&&<div className="verification-result" role="status"><h3>백업 무결성 인증을 통과했습니다.</h3><p>{result.files.toLocaleString('ko-KR')}개 파일을 인증했습니다. 데이터 복원은 실행하지 않았습니다.</p><p>새 작업 공간의 복호화 파일은 비공개로 보관하세요. 이 결과를 실제 복원 성공으로 사용하지 마세요.</p><details><summary>검사 식별 정보</summary><dl><div><dt>검사 ID</dt><dd>{result.id}</dd></div><div><dt>인증 manifest SHA-256</dt><dd><code>{result.authenticatedManifestSha256}</code></dd></div></dl></details></div>}

  <section aria-labelledby="external-check-heading"><h3 id="external-check-heading">보관한 사본으로 격리 DB 검사</h3>
   <p>인증한 사본을 별도 DB에 복원해 확인합니다. 원래 전시를 바꾸거나 전체 서비스를 복원하지 않습니다. 공개 서명 정보가 없는 과거 사본은 이 검사에서 사용할 수 없습니다.</p>
   <button type="button" disabled={!native||externalPending||pending||externalLoading} onClick={()=>void refreshExternal(true)}>인증·DB 검사 기록 새로고침</button>
   <label htmlFor="external-source">인증한 백업 사본</label><select id="external-source" value={selected?sourceId:''} disabled={!native||active||pending||externalPending} onChange={e=>{externalEpoch.current++;setSourceId(e.target.value);setResult(null);setError(null);setInput(previous=>({...previous,keyPath:''}));setExternalError(null);setExternalNotice('');}}>
    <option value="">확인할 사본을 선택하세요</option>{external.sources.map(source=><option key={source.verificationId} value={source.verificationId}>{new Date(source.at).toLocaleString('ko-KR')} · {source.backupId} · {source.files}개 파일</option>)}
   </select>
   {externalLoading&&<p role="status">인증·DB 검사 기록을 확인하고 있어요.</p>}
   {!externalLoading&&!external.sources.length&&<p className="note">먼저 위에서 백업 사본을 인증하세요. 기존 인증 기록이 원본과 연결되지 않았다면 같은 사본을 다시 인증합니다.</p>}
   {selected&&<><p className="note">선택한 사본과 별도로 보관한 키를 위의 키 파일 선택에서 지정하세요. 다시 열었을 때 키는 자동 선택하지 않습니다.</p><button type="button" disabled={disabled||!!externalError||!validVerificationInput({image:selected.image,keyPath:input.keyPath,sourcePath:input.keyPath})} onClick={()=>void externalAction({action:'check',verificationId:selected.verificationId,keyPath:input.keyPath})}>새 격리 DB 검사</button></>}
   {externalPending&&<p role="status">사본과 검사 도구를 확인하고 있습니다. 취소하려면 진행 중 백업·복원 취소를 사용하세요. 창을 닫는 것으로 취소되지 않습니다.</p>}
   {externalNotice&&<p role="status">{externalNotice}</p>}
   {externalError&&<div role="alert" className="alert"><p>{externalError.guidance}</p><p className="code">확인 코드: {externalError.code}</p><p>기존 사본·키와 검사 후보는 보존합니다. 기록을 새로고침하고 중단된 검사 도구 상태를 확인한 뒤 새 후보로 다시 검사하세요.</p></div>}
   <ol className="job-list" aria-label="격리 DB 검사 기록">{[...external.checks].reverse().map(job=><li key={job.id}><strong>{job.state==='completed'?'격리 DB 검사 완료':job.state==='running'?'격리 DB 검사 중':job.state==='interrupted'?'격리 DB 검사 중단':'격리 DB 검사 실패'}</strong><p className="code">검사 ID: {job.id}</p><p>사본: {job.expectedBackupId}</p><p>{job.state==='completed'?'별도 DB 복원과 비교를 확인했습니다. 전체 서비스·전시 공개 복원 완료가 아닙니다.':job.state==='running'?'현재 검사 상태를 확인하고 있습니다.':'기존 후보를 보존합니다. 다시 검사는 새 후보를 만들며 중단한 SQL부터 이어가지 않습니다.'}</p>{job.errorCode&&<p className="code">확인 코드: {job.errorCode}</p>}{['failed','interrupted'].includes(job.state)&&<button type="button" disabled={!native||active||pending||externalPending} onClick={()=>void externalAction({action:'reconcile',jobId:job.id,preserveCandidates:true})}>남은 검사 도구 상태 확인</button>}</li>)}</ol>
  </section>
 </section>;
}
