// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {installationLabel,managerError,type InstallationContext,type ManagerError} from './lifecycle';
interface Props{native:boolean;active:boolean;loading:boolean;context:InstallationContext|null;historyError:ManagerError|null;change(target:string|null,preserveExisting:boolean):Promise<InstallationContext>}
export function InstallationSelection({native,active,loading,context,historyError,change}:Props){
 const [target,setTarget]=useState(''),[ack,setAck]=useState(false),[pending,setPending]=useState(false),[error,setError]=useState<ManagerError|null>(null);
 const working=useRef(false),live=useRef(true),errorElement=useRef<HTMLDivElement>(null);
 useEffect(()=>{live.current=true;return()=>{live.current=false;};},[]);
 useEffect(()=>{if(context){setTarget(context.activeId);setAck(false);}},[context?.selectionToken]);
 useEffect(()=>{if(error)errorElement.current?.focus();},[error]);
 const disabled=!native||active||loading||pending||!context||!!historyError||context.mode!=='managed';
 const selected=context?.installations.find(root=>root.id===context.activeId);
 async function submit(id:string|null){
  if(working.current||disabled||!ack)return;working.current=true;setPending(true);setError(null);
  try{await change(id,true);}catch(e){if(live.current)setError(managerError(e));}
  finally{working.current=false;if(live.current){setPending(false);setAck(false);}}
 }
 return <section className="panel installation-selection" aria-labelledby="installation-heading">
  <p className="eyebrow">YOUR LOCAL SPACES</p><h2 id="installation-heading">관리 공간 선택</h2>
  <p>새 복원 공간을 만들거나 이전 공간으로 돌아갈 수 있습니다. 선택은 앱을 다시 열어도 유지됩니다.</p>
  {selected&&<><p><strong>현재: {installationLabel(selected)}</strong>{!selected.available&&' · 폴더 확인 필요'}</p><details><summary>현재 저장 위치</summary><p className="installation-path">{selected.path}</p></details></>}
  <p id="installation-scope" className="note">전환은 기존 서버를 자동 정지하지 않고 데이터를 이동하거나 지우지 않습니다. 새 공간 생성 후 아래 백업 복원에서 별도로 복원하세요.</p>
  {!native&&<p className="note">데스크톱 앱에서 관리 공간을 선택할 수 있습니다.</p>}
  {context?.mode==='override'&&<p className="note">지정 개발 공간을 사용 중입니다. 이 실행에서는 관리 공간을 바꿀 수 없습니다.</p>}
  {context?.mode==='platform-unverified'&&<p className="note">이 운영체제의 관리 공간 전환은 아직 검증되지 않아 비활성화했습니다.</p>}
  {context?.errorCode&&<p role="status">현재 폴더를 사용할 수 없습니다. 기존 폴더를 보존한 채 새 복원 공간을 만들 수 있습니다. 확인 코드: {context.errorCode}</p>}
  {historyError&&<p role="alert">관리 공간 목록을 확인하지 못했습니다. 상태 다시 확인으로 목록을 먼저 확인하세요.</p>}
  <fieldset disabled={disabled} aria-describedby="installation-scope"><legend>보존할 관리 공간</legend>
   <label htmlFor="installation-choice">등록된 공간</label><select id="installation-choice" value={target} onChange={event=>{setTarget(event.target.value);setAck(false);setError(null);}}>
    {(context?.installations??[]).map(root=><option key={root.id} value={root.id} disabled={!root.available}>{installationLabel(root)}{!root.available?' · 사용할 수 없음':''}</option>)}
   </select>
   <label className="backup-ack"><input type="checkbox" checked={ack} onChange={event=>setAck(event.target.checked)}/><span>기존 데이터가 보존되고 기존 서버는 자동 정지되지 않는 것을 확인했습니다.</span></label>
   <div className="button-row"><button disabled={!ack||!target||target===context?.activeId||!context?.installations.find(root=>root.id===target)?.available} onClick={()=>void submit(target)}>선택한 공간 사용</button><button className="primary" disabled={!ack} onClick={()=>void submit(null)}>새 복원 공간 만들고 선택</button></div>
  </fieldset>
  {pending&&<div role="status"><p>선택 기록을 저장하고 실제 상태를 확인합니다.</p><progress aria-label="관리 공간 변경 중"/></div>}
  {error&&<div className="alert" role="alert" ref={errorElement} tabIndex={-1}><strong>{error.guidance}</strong><p className="code">확인 코드: {error.code}</p><p>기존 폴더와 새 후보를 보존했습니다. 상태를 다시 확인한 뒤 진행하세요.</p></div>}
 </section>;
}
