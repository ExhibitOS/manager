// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {managerError,restorationStageLabel,validRestorationInput,type ManagerError,type RestorationInput,type RestorationReceipt,type RestorationContext} from './lifecycle';
interface Props{native:boolean;active:boolean;engineReady:boolean;context:RestorationContext|null;historyError:ManagerError|null;restore(input:RestorationInput):Promise<RestorationReceipt>}
export function BackupRestoration({native,active,engineReady,context,historyError,restore}:Props){
 const [input,setInput]=useState<RestorationInput>({image:'',keyPath:'',sourcePath:'',port:4500,freshInstallationAccepted:false});
 const [result,setResult]=useState<RestorationReceipt|null>(null),[error,setError]=useState<ManagerError|null>(null),[pending,setPending]=useState(false);
 const live=useRef(true),working=useRef(false),errorElement=useRef<HTMLDivElement>(null);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 const disabled=!native||!engineReady||active||pending||context?.fresh!==true||!!historyError,receipt=result??context?.receipt,job=context?.job;
 function change<K extends keyof RestorationInput>(name:K,value:RestorationInput[K]){setInput(previous=>({...previous,[name]:value}));setResult(null);setError(null);}
 async function submit(event:React.SubmitEvent<HTMLFormElement>){
  event.preventDefault();if(disabled||working.current||!validRestorationInput(input))return;
  working.current=true;setPending(true);setResult(null);setError(null);
  try{const restored=await restore(input);if(live.current)setResult(restored);}
  catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current){setPending(false);setInput(previous=>({...previous,freshInstallationAccepted:false}));}}
 }
 return <section className="panel backup-verification backup-restoration backup-creation" aria-labelledby="restoration-heading">
  <p className="eyebrow">RECOVER THE EXHIBITION</p><h2 id="restoration-heading">백업에서 새 전시 복원</h2>
  <p>원래 계정·DB·작품·서명 설정과 실행 이미지를 되살리고 새 전시를 시작합니다. 기존 전시와 사본·외부 키는 보존합니다.</p>
  <div className="job-summary needs-attention" id="restoration-scope"><h3>비어 있는 설치 공간에서 시작하세요.</h3><p>이 관리 앱의 설치 공간이 비어 있을 때만 복원합니다. 기존 설치나 실패 후보를 덮어쓰지 않습니다. 원래 전시의 쓰기는 멈춘 채 보관하고, 새 전시가 정상인지 확인한 후 사용할 전시를 결정하세요.</p><p>성공하면 새 주소에서 전시가 시작됩니다. 기존 계정으로 로그인할 수 있습니다. 키를 잃어버렸다면 복원할 수 없습니다.</p></div>
  {!native?<p className="note">데스크톱 앱에서만 실제 복원을 실행할 수 있습니다.</p>:context&&!context.fresh&&!job?<p className="note">현재 설치 공간에 기존 파일이 있어 복원을 시작할 수 없습니다. 기존 파일을 지우지 말고 별도의 비어 있는 설치 공간을 준비하세요.</p>:!engineReady?<p className="note">Docker와 Compose를 시작하고 실행 도구를 다시 확인하세요.</p>:!context&&!pending&&!historyError?<p className="note">저장된 복원 상태와 빈 설치 공간을 확인하고 있습니다.</p>:null}
  <p className="note">현재 복원 경로는 macOS·Docker 기준입니다. Windows·Podman과 전체 작품 검증은 준비 중입니다. 실행 중 취소는 위의 취소 요청을 사용하며 자동 재개하지 않습니다.</p>
  <form onSubmit={event=>void submit(event)} aria-describedby="restoration-scope">
   <fieldset disabled={disabled}><legend>복원할 사본과 별도 키</legend>
    <label htmlFor="restoration-source">복원할 암호화 백업 폴더의 전체 경로</label><input id="restoration-source" value={input.sourcePath} onChange={event=>change('sourcePath',event.target.value)} maxLength={2048} autoComplete="off" spellCheck={false}/>
    <label htmlFor="restoration-key">복원에 사용할 외부 키 파일의 전체 경로</label><input id="restoration-key" value={input.keyPath} onChange={event=>change('keyPath',event.target.value)} maxLength={2048} autoComplete="off" spellCheck={false} aria-describedby="restoration-key-help"/>
    <p className="note" id="restoration-key-help">키 내용 대신 백업·설치 폴더 밖의 비공개 32-byte 키 파일 경로를 입력하세요. 입력은 이 화면에만 유지합니다.</p>
    <label htmlFor="restoration-port">새 전시의 로컬 포트</label><input id="restoration-port" type="number" min={1024} max={65535} step={1} value={Number.isFinite(input.port)?input.port:''} onChange={event=>change('port',event.target.value===''?NaN:Number(event.target.value))} aria-describedby="restoration-port-help"/>
    <p className="note" id="restoration-port-help">1024–65535 중 사용하지 않는 포트를 선택하세요. 새 주소는 내 컴퓨터에서만 열립니다.</p>
    <details open><summary>검증된 실행 패키지 설정</summary><label htmlFor="restoration-image">복원용 유지보수 이미지의 고정 ID</label><input id="restoration-image" value={input.image} onChange={event=>change('image',event.target.value)} maxLength={71} autoComplete="off" spellCheck={false}/><p className="note">신뢰할 운영자가 준비한 sha256: 형식의 로컬 이미지 ID를 사용합니다. 사본 인증은 배포 서명 확인을 대신하지 않습니다.</p></details>
    <label className="backup-ack"><input type="checkbox" checked={input.freshInstallationAccepted} onChange={event=>change('freshInstallationAccepted',event.target.checked)}/><span>원래 전시의 쓰기를 중지했으며, 비어 있는 설치 공간에 복원하고 새 전시를 시작하는 데 동의합니다.</span></label>
   </fieldset>
   <button className="primary" type="submit" disabled={disabled||!validRestorationInput(input)}>{pending?'복원과 전시 응답 확인 중…':'새 전시 복원과 시작'}</button>
  </form>
  {pending&&<div className="job-summary" role="status"><strong>사본을 복원하고 새 전시를 확인하고 있습니다.</strong><p>데이터 크기에 따라 시간이 걸립니다. 창을 유지하세요. 완료 전까지 다른 관리 작업은 실행하지 않습니다.</p><progress aria-label="새 전시 복원 진행 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>완료 여부가 확인되지 않았습니다. 저장된 복원 기록을 다시 확인하세요. 원본·키·새 후보는 보존되며 실패한 부분 DB에 다시 실행하지 않습니다.</p></div>}
  {receipt&&<div className="verification-result" role="status"><h3>새 전시 복원과 시작을 확인했습니다.</h3><p>백업의 DB·작품·설정을 비교하고 전시 응답을 확인한 기록입니다. 현재 응답은 위의 전시 상태에서 다시 확인한 뒤 전시 열기를 누르세요.</p><p>기존 관리자 계정과 비밀번호를 사용합니다. 원본 사본과 외부 키는 계속 별도로 보관하세요.</p><details><summary>복원 식별 정보</summary><dl><div><dt>새 전시 주소</dt><dd>{receipt.openUrl}</dd></div><div><dt>복원 작업 ID</dt><dd>{receipt.id}</dd></div><div><dt>백업 ID</dt><dd>{receipt.backupId}</dd></div><div><dt>인증 manifest SHA-256</dt><dd><code>{receipt.authenticatedManifestSha256}</code></dd></div></dl></details></div>}
  <div className="backup-history" role="region" aria-label="저장된 복원 기록"><h3>복원 기록</h3>
   {historyError?<div className="alert" role="alert"><strong>저장된 복원 상태를 확인할 수 없습니다.</strong><p>실행 도구 다시 확인을 눌러 기록을 읽으세요. 확인 전에는 복원·설치·시작을 중지합니다.</p><p className="code">확인 코드: {historyError.code}</p></div>:job?<div className="job-summary"><strong>{{running:'복원 진행 중',completed:'복원 완료 기록',failed:'복원 실패',interrupted:'복원 중단'}[job.state]}</strong><p>{restorationStageLabel(job.stage)}</p><p className="code">작업 ID: {job.id}{job.errorCode&&` · 확인 코드: ${job.errorCode}`}</p><time dateTime={new Date(job.updatedAt).toISOString()}>{new Date(job.updatedAt).toLocaleString('ko-KR')}</time>{['failed','interrupted'].includes(job.state)&&<p>실패·중단 후보와 데이터는 보존됩니다. 남은 helper를 작업 ID와 정확한 label로 조사하고 새로운 빈 설치 공간에서 복원하세요. 일반 다시 시도나 자동 취소·재개는 실행하지 않습니다. 필요한 경우 위의 정지로 새 후보만 정지할 수 있습니다.</p>}</div>:<p className="note">{context?'저장된 복원 기록이 없습니다.':'저장된 상태를 아직 확인하지 못했습니다.'}</p>}
  </div>
 </section>;
}
