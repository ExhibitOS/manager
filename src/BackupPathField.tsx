// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {client,managerError} from './lifecycle';
export function BackupPathField({id,label,kind,value,onChange,disabled,helpId,allowCreate=false}:{id:string;label:string;kind:'key'|'archive';value:string;onChange(value:string):void;disabled:boolean;helpId?:string;allowCreate?:boolean}){
 const [pending,setPending]=useState(false),[failure,setFailure]=useState<string|null>(null),epoch=useRef(0),live=useRef(true),working=useRef(false);
 useEffect(()=>{live.current=true;return()=>{live.current=false;epoch.current++;};},[]);
 useEffect(()=>{epoch.current++;setFailure(null);},[value,disabled]);
 async function choose(create=false){
  if(disabled||working.current||!client.native)return;
  working.current=true;
  const request=++epoch.current;setPending(true);setFailure(null);
  try{const path=await client.chooseBackupPath(create?'new_key':kind);if(live.current&&request===epoch.current&&path!==null)onChange(path);}
  catch(error){if(live.current&&request===epoch.current)setFailure(managerError(error).guidance);}
  finally{working.current=false;if(live.current)setPending(false);}
 }
 return <div className="backup-path-field"><button type="button" onClick={()=>void choose()} disabled={disabled||pending||!client.native} aria-describedby={helpId}>{pending?'선택 창을 확인하세요…':kind==='key'?'백업 키 파일 선택':'백업 폴더 선택'}</button>
  {allowCreate&&kind==='key'&&<button type="button" onClick={()=>void choose(true)} disabled={disabled||pending||!client.native} aria-describedby={helpId}>새 백업 키 만들기</button>}
  <p className="note" role="status">{value?`선택됨: ${value.split(/[\\/]/).filter(Boolean).at(-1)}`:kind==='key'?'백업과 별도로 보관한 키 파일을 선택하세요.':'복원하거나 확인할 암호화 백업 폴더를 선택하세요.'}</p>
  {failure&&<p role="alert">{failure}</p>}
  <details><summary>경로 직접 입력</summary><label htmlFor={id}>{label}</label><input id={id} value={value} onChange={event=>onChange(event.target.value)} disabled={disabled||pending} maxLength={2048} autoComplete="off" spellCheck={false} aria-describedby={helpId}/></details>
 </div>;
}
