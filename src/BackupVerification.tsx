// SPDX-License-Identifier: Apache-2.0
import {BackupPathField} from './BackupPathField';
import {BackupToolField,type BackupToolContext} from './BackupToolField';
import {useEffect,useRef,useState} from 'react';
import {managerError,validVerificationInput,type ManagerError,type VerificationInput,type VerificationReceipt} from './lifecycle';
interface Props{savedBackup?:{path:string;keyPath:string}|null;tool:BackupToolContext;native:boolean;installed:boolean;active:boolean;engineReady:boolean;verify(input:VerificationInput):Promise<VerificationReceipt>}
export function BackupVerification({savedBackup,tool,native,installed,active,engineReady,verify}:Props){
 const [input,setInput]=useState<VerificationInput>({image:'',keyPath:'',sourcePath:''});
 const [result,setResult]=useState<VerificationReceipt|null>(null),[error,setError]=useState<ManagerError|null>(null),[pending,setPending]=useState(false);
 const errorElement=useRef<HTMLDivElement>(null),working=useRef(false),live=useRef(true);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 useEffect(()=>{if(savedBackup){setInput(previous=>({...previous,sourcePath:savedBackup.path,...(savedBackup.keyPath?{keyPath:savedBackup.keyPath}:{})}));setResult(null);setError(null);}},[savedBackup]);
 const unavailable=!native||!installed||!engineReady,disabled=unavailable||active||pending;
 function change(name:keyof VerificationInput,value:string){setInput(previous=>({...previous,[name]:value}));setResult(null);setError(null);}
 async function submit(event:React.SubmitEvent<HTMLFormElement>){
  event.preventDefault();if(working.current||disabled||!validVerificationInput(input))return;
  working.current=true;setPending(true);setError(null);setResult(null);
  try{const receipt=await verify(input);if(live.current)setResult(receipt);}catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current)setPending(false);}
 }
 return <section className="panel backup-verification" aria-labelledby="backup-heading">
  <p className="eyebrow">PRESERVE & VERIFY</p><h2 id="backup-heading">백업 사본 검증</h2>
  <p>암호화된 백업이 손상되지 않았는지 확인합니다. 원본과 키는 읽기만 하며, 새 비공개 작업 공간에 복호화된 파일이 남습니다.</p>
  <p className="note" id="backup-scope">이 검사는 DB 복원·서비스 활성화를 수행하지 않습니다. 복원할 수 있는지 확인하려면 별도 새 환경 복원이 필요합니다.</p>
  {unavailable&&<p className="note">{!native?'데스크톱 앱에서만 검증할 수 있습니다.':!installed?'전시 설치와 실행 도구 확인을 먼저 완료하세요.':'설치에 사용한 실행 도구를 시작하고 상태를 확인하세요.'}</p>}
  <form onSubmit={event=>void submit(event)} aria-describedby="backup-scope">
   <fieldset disabled={disabled}><legend>준비한 백업과 외부 키</legend>
    <BackupPathField id="backup-source" label="암호화된 백업 폴더의 전체 경로" kind="archive" value={input.sourcePath} onChange={value=>change('sourcePath',value)} disabled={disabled} helpId="backup-path-help"/>
    <BackupPathField id="backup-key" label="별도로 보관한 키 파일의 전체 경로" kind="key" value={input.keyPath} onChange={value=>change('keyPath',value)} disabled={disabled} helpId="backup-key-help"/>
    <p className="note" id="backup-key-help">키 내용은 입력하지 마세요. 백업 폴더 밖의 비공개 키 파일을 사용합니다.</p>
    {savedBackup&&input.sourcePath===savedBackup.path&&<p className="note" role="status">방금 외부에 보관한 사본을 선택했습니다. 키를 확인한 뒤 검증하세요. 복원은 실행하지 않습니다.</p>}
    <p className="note" id="backup-path-help">파일 선택 창에서 암호화 백업 폴더를 선택하세요. 필요한 경우 경로 직접 입력을 사용할 수 있습니다.</p>
    <BackupToolField id="backup-image" label="유지보수 이미지의 고정 ID" value={input.image} onChange={value=>change('image',value)} context={tool}/>
   </fieldset>
   <button className="primary" type="submit" disabled={disabled||!validVerificationInput(input)}>{pending?'백업 인증 확인 중…':'백업 사본 검증'}</button>
  </form>
  {pending&&<div className="job-summary" role="status"><strong>백업 도구를 준비하고 암호화 사본을 확인하고 있습니다.</strong><p>검사가 끝날 때까지 창을 유지하세요. 설치·시작 등 다른 작업은 함께 실행하지 않습니다.</p><progress aria-label="백업 인증 검사 진행 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>입력과 실행 도구 상태를 확인한 뒤 다시 검증하세요. 기존 사본과 키는 보존되며, 실패한 새 작업 공간에는 부분 복호화 파일이 남을 수 있습니다.</p></div>}
  {result&&<div className="verification-result" role="status"><h3>백업 무결성 인증을 통과했습니다.</h3><p>{result.files.toLocaleString('ko-KR')}개 파일을 인증했습니다. 데이터 복원은 실행하지 않았습니다.</p><p>새 작업 공간의 복호화 파일은 비공개로 보관하세요. 이 결과를 실제 복원 성공으로 사용하지 마세요.</p><details><summary>검사 식별 정보</summary><dl><div><dt>검사 ID</dt><dd>{result.id}</dd></div><div><dt>인증 manifest SHA-256</dt><dd><code>{result.authenticatedManifestSha256}</code></dd></div></dl></details></div>}
 </section>;
}
