// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {backupStageLabel,managerError,validCreationInput,type BackupJob,type CreationInput,type CreationReceipt,type ManagerError} from './lifecycle';
interface Props{native:boolean;installed:boolean;active:boolean;engineReady:boolean;jobs:BackupJob[];historyError:ManagerError|null;create(input:CreationInput):Promise<CreationReceipt>}
export function BackupCreation({native,installed,active,engineReady,jobs,historyError,create}:Props){
 const [input,setInput]=useState<CreationInput>({image:'',keyPath:'',externalWritersQuiesced:false,downtimeAccepted:false});
 const [result,setResult]=useState<CreationReceipt|null>(null),[error,setError]=useState<ManagerError|null>(null),[pending,setPending]=useState(false);
 const working=useRef(false),live=useRef(true),errorElement=useRef<HTMLDivElement>(null);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 const unavailable=!native||!installed||!engineReady,disabled=unavailable||active||pending||!!historyError;
 function change<K extends keyof CreationInput>(name:K,value:CreationInput[K]){setInput(previous=>({...previous,[name]:value}));setResult(null);setError(null);}
 async function submit(event:React.SubmitEvent<HTMLFormElement>){
  event.preventDefault();if(working.current||disabled||!validCreationInput(input))return;
  working.current=true;setPending(true);setResult(null);setError(null);
  try{const receipt=await create(input);if(live.current)setResult(receipt);}catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current){setPending(false);setInput(previous=>({...previous,externalWritersQuiesced:false,downtimeAccepted:false}));}}
 }
 return <section className="panel backup-verification backup-creation" aria-labelledby="creation-heading">
  <p className="eyebrow">PRESERVE THE INSTALLATION</p><h2 id="creation-heading">설치된 전시 백업</h2>
  <p>DB・작품・서명 설정・설치 파일과 실행 이미지를 암호화하고 사본을 인증합니다. 원본을 지우지 않으며, 키는 사본과 별도로 보관해야 합니다.</p>
  <div className="job-summary needs-attention" id="creation-scope"><h3>백업 중 전시가 중단됩니다.</h3><p>Manager가 전시의 쓰기를 멈춥니다. 다른 앱·worker·스크립트의 쓰기는 직접 멈춰야 합니다. 성공하거나 실패한 뒤에도 전시는 정지 상태로 남을 수 있습니다.</p><p>결과와 남은 유지보수 작업을 확인한 다음 위의 시작 버튼으로 명시적으로 재개하세요. 사본 생성은 새 환경에서의 복원 성공을 의미하지 않습니다.</p></div>
  {unavailable&&!pending&&<p className="note">{!native?'데스크톱 앱에서만 백업을 생성할 수 있습니다.':!installed?'전시 설치 상태와 실행 도구를 먼저 확인하세요.':'설치에 사용한 실행 도구를 시작하고 상태를 확인하세요.'}</p>}
  <p className="note">현재 생성 경로는 Docker 기반 설치를 지원합니다. 실제 검증은 macOS에서 수행했으며 Windows·Podman 검증은 아직 남아 있습니다.</p>
  <form onSubmit={event=>void submit(event)} aria-describedby="creation-scope">
   <fieldset disabled={disabled}><legend>외부 키와 백업 준비</legend>
    <label htmlFor="creation-key">생성에 사용할 외부 키 파일의 전체 경로</label>
    <input id="creation-key" value={input.keyPath} onChange={event=>change('keyPath',event.target.value)} maxLength={2048} autoComplete="off" spellCheck={false} aria-describedby="creation-key-help"/>
    <p className="note" id="creation-key-help">키 내용은 입력하지 마세요. 설치 폴더 밖의 32-byte 비공개 키 파일(0600)을 사용합니다. 경로와 입력은 현재 화면에만 유지하며 키 분실 시 복원이 불가능합니다.</p>
    <details open><summary>검증된 실행 패키지 설정</summary><label htmlFor="creation-image">백업 생성용 유지보수 이미지의 고정 ID</label>
     <input id="creation-image" value={input.image} onChange={event=>change('image',event.target.value)} maxLength={71} autoComplete="off" spellCheck={false} aria-describedby="creation-image-help"/>
     <p className="note" id="creation-image-help">신뢰할 운영자가 준비한 sha256: 형식의 로컬 이미지 ID를 사용합니다. 새 이미지를 자동으로 내려받거나 배포 서명을 검증하지 않습니다.</p>
    </details>
    <label className="backup-ack"><input type="checkbox" checked={input.externalWritersQuiesced} onChange={event=>change('externalWritersQuiesced',event.target.checked)}/><span>다른 앱·worker·스크립트의 DB·작품·설정 쓰기를 중지했습니다.</span></label>
    <label className="backup-ack"><input type="checkbox" checked={input.downtimeAccepted} onChange={event=>change('downtimeAccepted',event.target.checked)}/><span>전시가 중단되며, 결과 확인 후 직접 다시 시작하는 데 동의합니다.</span></label>
   </fieldset>
   <button className="primary" type="submit" disabled={disabled||!validCreationInput(input)}>{pending?'백업 생성과 인증 중…':'전시를 멈추고 백업 생성'}</button>
  </form>
  {pending&&<div className="job-summary" role="status"><strong>백업을 만들고 사본을 인증하고 있습니다.</strong><p>완료까지 걸리는 시간은 데이터 크기에 따라 다릅니다. 진행률을 추측하지 않습니다. 창을 유지하세요. 설치·시작·검증 등 다른 작업은 함께 실행하지 않습니다.</p><progress aria-label="백업 생성과 인증 진행 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>원본과 키, 새 사본·작업 공간은 보존됩니다. 전시가 정지 상태일 수 있습니다. 남은 helper와 작업 기록을 확인하고 외부 쓰기 중지·전시 중단을 다시 확인한 뒤 새 백업을 생성하세요.</p></div>}
  {result&&<div className="verification-result" role="status"><h3>암호화 백업 생성과 인증을 완료했습니다.</h3><p>{result.files.toLocaleString('ko-KR')}개 파일을 인증하고 암호화 사본을 보관했습니다. 전시는 정지 상태로 남습니다. 데이터 복원은 실행하지 않았습니다.</p><p>사본은 설치 폴더 안의 <code>backup-creation-{result.id}/archive/</code>에 보관됩니다. 이 폴더와 키를 서로 분리된 비공개 위치에 추가 보관하세요. 생성 작업 volume에는 민감한 인증 검사의 복호화 파일이 남습니다. 자동 삭제는 없습니다.</p><details><summary>생성 식별 정보</summary><dl><div><dt>작업 ID</dt><dd>{result.id}</dd></div><div><dt>백업 ID</dt><dd>{result.backupId}</dd></div><div><dt>인증 manifest SHA-256</dt><dd><code>{result.authenticatedManifestSha256}</code></dd></div></dl></details></div>}
  <div className="backup-history" role="region" aria-label="저장된 백업 생성 기록"><h3>백업 생성 기록</h3>
   {historyError?<div className="alert" role="alert"><strong>저장된 백업 작업을 확인할 수 없습니다.</strong><p>상태 다시 확인으로 기록을 읽기 전까지 새 생성은 중지합니다. 완료 상태를 추측하지 않습니다.</p><p className="code">확인 코드: {historyError.code}</p></div>:jobs.length?<ol className="job-list">{[...jobs].reverse().slice(0,10).map(job=><li key={job.id}><strong>{{running:'진행 중',completed:'생성 완료',failed:'실패',interrupted:'중단됨'}[job.state]}</strong><span>{backupStageLabel(job.stage)}</span><time dateTime={new Date(job.updatedAt).toISOString()}>{new Date(job.updatedAt).toLocaleString('ko-KR')}</time><p className="code">작업 ID: {job.id}{job.errorCode&&` · 확인 코드: ${job.errorCode}`}</p>{job.state==='running'&&<p>다른 작업이 실행 중입니다. 완료 전까지 설치·시작·검증·새 백업을 실행하지 않습니다.</p>}{['failed','interrupted'].includes(job.state)&&<p>완료된 사본으로 표시하지 않습니다. 원본·실패 후보와 volume은 보존됩니다. 중단된 작업은 ID와 정확한 label로 남은 helper를 조사한 뒤 새로 생성하세요. 자동 재개·취소 기능은 아직 없습니다.</p>}</li>)}</ol>:<p className="note">저장된 생성 기록이 없습니다. 실제 인증을 마친 사본만 생성 완료로 기록합니다.</p>}
  </div>
 </section>;
}
