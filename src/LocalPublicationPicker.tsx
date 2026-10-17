// SPDX-License-Identifier: Apache-2.0
import {useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import type {HostedSource} from './hosted-deployment';
type Publication={publicationId:string;exhibitionId:string;title:string;revisionSha256:string;publishedAt:string;status:'published'};
export function LocalPublicationPicker({selectionToken,disabled,onSource,onBusy}:{selectionToken?:string;disabled:boolean;onSource:(source:HostedSource)=>void;onBusy:(busy:boolean)=>void}){
 const [items,setItems]=useState<Publication[]>([]),[selected,setSelected]=useState(''),[busy,setBusy]=useState<'catalog'|'export'|null>(null),[loaded,setLoaded]=useState(false),[error,setError]=useState('');
 async function run(action:'catalog'|'export'){
  if(!selectionToken||busy)return;setBusy(action);onBusy(true);setError('');
  try{
   if(action==='catalog'){setItems([]);setSelected('');setLoaded(false);const result=await invoke<{items:Publication[]}>('manager_publication_catalog',{selectionToken});setItems(result.items);setLoaded(true);if(result.items.length===1)setSelected(result.items[0]!.publicationId);}
   else{const source=await invoke<HostedSource>('manager_publication_export',{selectionToken,publicationId:selected});onSource(source);}
  }catch(e){const code=typeof e==='object'&&e&&'code'in e?String(e.code):'';const reason=code==='LOCAL_CMS_AUTH_REQUIRED'||code==='LOCAL_CMS_CREDENTIALS_INVALID'?'로컬 Studio 연결 정보를 확인하세요.':code==='LOCAL_CMS_PERMISSION_REQUIRED'?'이 계정에서 공개한 전시만 선택할 수 있습니다.':code==='LOCAL_CMS_SOURCE_CHANGED'?'공개한 내용이 바뀌었습니다. 목록을 다시 불러와 주세요.':'로컬 전시가 실행 중이고 Studio에서 공개됐는지 확인하세요.';setError(`${reason} 기존 전시 파일은 그대로입니다.`);}
  finally{setBusy(null);onBusy(false);}
 }
 return <div className="local-publication-picker"><p>이 Mac의 Studio에서 공개한 전시를 선택하세요. 계정 정보는 화면에 표시하거나 외부로 보내지 않습니다.</p><button type="button" disabled={disabled||!!busy||!selectionToken} onClick={()=>void run('catalog')}>로컬 공개 전시 불러오기</button>{loaded&&items.length===0&&<p role="status">공개한 전시가 없습니다. Studio에서 전시를 공개한 뒤 다시 불러오세요.</p>}{items.length>0&&<><label>공개할 전시<select value={selected} disabled={disabled||!!busy} onChange={e=>setSelected(e.target.value)}><option value="">전시를 선택하세요</option>{items.map(item=><option key={item.publicationId} value={item.publicationId}>{item.title} · {new Date(item.publishedAt).toLocaleDateString()}</option>)}</select></label><button type="button" disabled={disabled||!!busy||!selected} onClick={()=>void run('export')}>선택한 전시 준비</button></>}{busy&&<div role="status"><progress aria-label={busy==='catalog'?'전시 목록 불러오는 중':'공개 전시 파일 준비 중'}/><p>{busy==='catalog'?'공개한 전시 목록을 불러오고 있습니다…':'공개 권리와 파일을 확인하고 전시 묶음을 만들고 있습니다…'} 이 단계에서는 클라우드에 올리지 않습니다.</p></div>}{error&&<p role="alert">{error}</p>}</div>;
}
