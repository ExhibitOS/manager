// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {BackupPathField} from './BackupPathField';
import {usePreparationBoundary,usePreparationRecord} from './PreparationStartup';
import {preparationClient,preparationGuidance,rememberPreparationView,preparationPhaseLabel,preparationStateLabel,validPreparationPath,type PreparationInspection} from './update-preparation';
export function UpdatePreparation({native,selectionToken,installationId,active,onActivity}:{native:boolean;selectionToken:string;installationId:string|null;active:boolean;onActivity:(active:boolean)=>void}){
 const [releaseFolder,setReleaseFolder]=useState(''),[keyFile,setKeyFile]=useState(''),[inspection,setInspection]=useState<PreparationInspection|null>(null);
 const [writers,setWriters]=useState(false),[consent,setConsent]=useState(false),[pending,setPending]=useState<'choose'|'inspect'|'dispatch'|null>(null),[error,setError]=useState<string|null>(null);
 const live=useRef(true),epoch=useRef(0),working=useRef(false),current=useRef({selectionToken,installationId});current.current={selectionToken,installationId};
 const block=usePreparationBoundary(),latest=usePreparationRecord();
 const previous=latest?.installationId===installationId?latest:null;
 useEffect(()=>{live.current=true;return()=>{live.current=false;epoch.current++;};},[]);
 function invalidate(){epoch.current++;setInspection(null);setConsent(false);setWriters(false);setError(null);}
 useEffect(()=>{epoch.current++;setReleaseFolder('');setKeyFile('');setInspection(null);setConsent(false);setWriters(false);setError(null);},[selectionToken,installationId]);
 const disabled=!native||active||!installationId||pending!==null;
 function stillCurrent(run:number,token:string,id:string|null){return live.current&&epoch.current===run&&current.current.selectionToken===token&&current.current.installationId===id;}
 async function choose(){
  if(disabled||working.current)return;invalidate();const run=epoch.current,token=selectionToken,id=installationId;working.current=true;setPending('choose');
  try{const path=await preparationClient.chooseRelease(token);if(stillCurrent(run,token,id)&&path)setReleaseFolder(path);}
  catch{if(stillCurrent(run,token,id))setError('업데이트 폴더를 선택하지 못했습니다. 선택 창과 현재 관리 공간을 확인하세요.');}
  finally{working.current=false;if(live.current)setPending(null);}
 }
 async function inspect(){
  if(disabled||working.current||!validPreparationPath(releaseFolder)||!validPreparationPath(keyFile))return;
  invalidate();const run=epoch.current,token=selectionToken,id=installationId;working.current=true;setPending('inspect');
  try{const value=await preparationClient.inspect({selectionToken:token,releaseFolder,keyFile});if(stillCurrent(run,token,id))setInspection(value);}
  catch{if(stillCurrent(run,token,id))setError('준비 자료를 확인하지 못했습니다. 폴더·키·준비 도구를 확인한 뒤 다시 검사하세요.');}
  finally{working.current=false;if(live.current)setPending(null);}
 }
 async function dispatch(){
  if(disabled||working.current||!inspection?.available||!consent||!writers)return;
  const token=selectionToken,id=installationId;working.current=true;setPending('dispatch');setError(null);rememberPreparationView();onActivity(true);
  try{
   const job=await preparationClient.dispatch({selectionToken:token,releaseFolder,keyFile,preserveData:true,externalWritersQuiesced:true,closeAppAndPrepare:true});
   if(job.installationId!==id)throw Error('MANAGER_PROTOCOL');
   // A dispatched handoff must block normal effects even if the view selection changed meanwhile.
   block(job);
  }catch{block(null);}
  finally{working.current=false;if(live.current)setPending(null);}
 }
 return <section className="backup-verification backup-creation" aria-label="업데이트 준비"><h3>업데이트 준비</h3>
  {previous&&<div className="lifecycle-feedback" role="status"><strong>마지막 준비 기록: {preparationStateLabel[previous.state]}</strong><p>{preparationPhaseLabel[previous.phase]}</p>{previous.state==='prepared'?<p>아래의 준비된 업데이트 적용에서 자료를 다시 확인하고 적용을 따로 선택하세요.</p>:previous.code&&<p>{preparationGuidance(previous.code)}</p>}<details><summary>준비 기록 정보</summary><p className="code">작업 ID: {previous.id}</p>{previous.code&&<p className="code">확인 코드: {previous.code}</p>}</details></div>}<p>업데이트 폴더와 백업 키를 확인한 뒤, 앱을 닫고 별도 공간에서 복구·업데이트 검사를 진행합니다. 준비 완료 후 적용을 따로 선택합니다.</p>
  <fieldset disabled={disabled}><legend>1. 준비 자료 선택</legend><button type="button" onClick={()=>void choose()}>업데이트 폴더 선택</button>
   <p role="status">{releaseFolder?`선택한 폴더: ${releaseFolder.split(/[\\/]/).filter(Boolean).at(-1)}`:'선택한 업데이트 폴더가 없습니다.'}</p>
   <details><summary>업데이트 폴더 경로 직접 입력</summary><label htmlFor="prepare-release">업데이트 폴더 전체 경로</label><input id="prepare-release" value={releaseFolder} onChange={e=>{invalidate();setReleaseFolder(e.target.value);}} autoComplete="off" spellCheck={false}/></details>
   <BackupPathField id="prepare-key" label="별도로 보관한 백업 키 파일" kind="key" value={keyFile} onChange={path=>{invalidate();setKeyFile(path);}} disabled={disabled} allowCreate/>
   <button type="button" disabled={disabled||!validPreparationPath(releaseFolder)||!validPreparationPath(keyFile)} onClick={()=>void inspect()}>준비 자료 검사</button>
  </fieldset>
  {pending&&<div className="lifecycle-feedback" role="status" aria-live="polite"><strong>{pending==='choose'?'폴더 선택 창을 확인하세요':pending==='inspect'?'준비 자료를 확인하고 있습니다':'준비 요청을 저장하고 있습니다'}</strong><progress aria-label="업데이트 준비 응답 대기"/></div>}
  {error&&<p className="alert" role="alert">{error}</p>}
  {inspection&&<div role="status" aria-live="polite"><p>{inspection.available?'선택한 자료로 준비를 요청할 수 있습니다. 전체 검사는 앱을 닫은 뒤 진행합니다.':preparationGuidance(inspection.code)}</p>{inspection.version&&<p>업데이트 버전: {inspection.version}</p>}{inspection.changedSchema!==null&&<p>{inspection.changedSchema?'데이터 구조 변경이 포함됩니다. 복구 자료를 포함해 검사합니다.':'데이터 구조 변경 여부를 확인했습니다.'}</p>}{inspection.code&&<details><summary>검사 정보</summary><p className="code">확인 코드: {inspection.code}</p></details>}</div>}
  {inspection?.available&&<fieldset disabled={disabled}><legend>2. 중단 영향 확인</legend>
   <label className="backup-ack"><input type="checkbox" checked={writers} onChange={e=>setWriters(e.target.checked)}/>외부에서 이 전시에 쓰는 작업을 중지했습니다. 실제 실행 상태도 다시 검사합니다.</label>
   <label className="backup-ack"><input type="checkbox" checked={consent} onChange={e=>setConsent(e.target.checked)}/>기존 데이터·백업·키를 보존하고 앱을 닫아 준비합니다. 검사 중 전시가 중단될 수 있습니다.</label>
   <button className="primary" type="button" disabled={disabled||!writers||!consent} onClick={()=>void dispatch()}>앱을 닫고 업데이트 준비</button>
  </fieldset>}
 </section>;
}
