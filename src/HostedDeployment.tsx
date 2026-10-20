// SPDX-License-Identifier: Apache-2.0
import {useEffect,useRef,useState} from 'react';
import {LocalPublicationPicker} from './LocalPublicationPicker';
import {HostedRecovery} from './HostedRecovery';
import {hostedClient,hostedFailure,hostedUncertain,type HostedProvider,type HostedAccount,type HostedReceipt,type HostedSource,type HostedResource,type HostedResume} from './hosted-deployment';
const name=(provider:HostedProvider)=>provider==='cloudflare'?'Cloudflare':'Render';
function ProviderConnection({provider,disabled,onAccount}:{provider:HostedProvider;disabled:boolean;onAccount:(id:string)=>void}){
 const [token,setToken]=useState(''),[stored,setStored]=useState(false),[accounts,setAccounts]=useState<HostedAccount[]>([]),[selected,setSelected]=useState(''),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{if(hostedClient.native())void hostedClient.connected(provider).then(setStored).catch(()=>{});},[provider]);
 async function verify(save:boolean){
  const password=token.trim();setToken('');setBusy(true);setError('');setAccounts([]);setSelected('');onAccount('');
  try{if(save){await hostedClient.store(provider,password);setStored(true);}const result=await hostedClient.operation({provider,action:'accounts'});const list=result.accounts??[];setAccounts(list);const id=list.length===1?list[0]!.id:'';setSelected(id);onAccount(id);if(!list.length)setError(provider==='cloudflare'?'수락한 계정 멤버십을 찾지 못했습니다. 계정 초대 수락 상태와 토큰의 사용자 멤버십 읽기 권한을 확인하세요.':'접근할 수 있는 워크스페이스가 없습니다. 토큰의 계정 권한을 확인하세요.');}
  catch(e){const code=hostedFailure(e);setError(provider==='cloudflare'&&code==='HOSTED_PERMISSION_REQUIRED'?`토큰에 사용자 멤버십 읽기와 선택 계정의 Cloudflare Pages 편집 권한을 허용하세요. 확인 코드: ${code}`:`연결을 확인하지 못했습니다. 토큰 권한을 확인하세요. 확인 코드: ${code}`);}finally{setBusy(false);}
 }
 async function disconnect(){setBusy(true);setError('');try{await hostedClient.disconnect(provider);setStored(false);setAccounts([]);setSelected('');setToken('');onAccount('');}catch(e){setError(`연결 해제를 완료하지 못했습니다. 확인 코드: ${hostedFailure(e)}`);}finally{setBusy(false);}}
 return <fieldset disabled={disabled||busy} className="hosted-connection"><legend>{name(provider)} 연결</legend>
 <p>{provider==='cloudflare'?'전시 파일을 Pages에 올립니다.':'API 자원을 준비할 때만 연결합니다. 전시 데이터 이전·연결은 아직 완료되지 않습니다.'} 토큰은 이 Mac의 키체인에 보관합니다.</p>
 {provider==='cloudflare'&&<details><summary>필요한 토큰 권한</summary><p>사용자 → Memberships → Read, 선택한 계정 → Cloudflare Pages → Edit를 허용하세요. 계정 목록이 보여도 Pages 편집 권한은 배포할 때 별도로 확인합니다.</p></details>}
 <div className="button-row"><button type="button" onClick={()=>void hostedClient.link(`${provider}-signup`).catch(()=>setError('가입 페이지를 열지 못했습니다.'))}>계정 만들기</button><button type="button" onClick={()=>void hostedClient.link(`${provider}-token`).catch(()=>setError('토큰 설정 페이지를 열지 못했습니다.'))}>토큰 발급 안내</button></div>
 <label>{stored?'토큰 교체 (선택)':'API 토큰'}<input type="password" value={token} maxLength={4096} autoComplete="off" autoCapitalize="none" spellCheck={false} onChange={e=>setToken(e.target.value)} aria-label={`${name(provider)} API 토큰`}/></label>
 <div className="button-row"><button type="button" disabled={!token.trim()} onClick={()=>void verify(true)}>키체인에 보관하고 연결 확인</button>{stored&&<><button type="button" onClick={()=>void verify(false)}>보관한 토큰으로 연결 확인</button><button type="button" onClick={()=>void disconnect()}>연결 해제</button></>}</div>
 {stored&&!accounts.length&&<p>이 기기에 토큰이 보관돼 있습니다. 계정 접근은 연결 확인 후 표시합니다.</p>}
 {accounts.length>0&&<label>{provider==='cloudflare'?'사용할 계정':'사용할 워크스페이스'}<select value={selected} onChange={e=>{setSelected(e.target.value);onAccount(e.target.value);}}><option value="">선택하세요</option>{accounts.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</select></label>}
 {busy&&<p role="status">{name(provider)} 계정 접근을 확인하고 있습니다…</p>}{error&&<p role="alert">{error}</p>}
 </fieldset>;
}
export function HostedDeployment({selectionToken}:{selectionToken?:string}){
 const [recoveryProject,setRecoveryProject]=useState(''),[recoveryConnection,setRecoveryConnection]=useState(''),[recoveryGeneration,setRecoveryGeneration]=useState(0);
 const native=hostedClient.native(),[mode,setMode]=useState<'static'|'api'>('static'),[cloudAccount,setCloudAccount]=useState(''),[workspace,setWorkspace]=useState(''),[source,setSource]=useState<HostedSource|null>(null),[publicConsent,setPublicConsent]=useState(false),[costConsent,setCostConsent]=useState(false),[serviceId,setServiceId]=useState(''),[postgresId,setPostgresId]=useState(''),[pending,setPending]=useState(false),[localBusy,setLocalBusy]=useState(false),[receipt,setReceipt]=useState<HostedReceipt|null>(null),[apiReceipt,setApiReceipt]=useState<HostedReceipt|null>(null),[error,setError]=useState(''),[services,setServices]=useState<HostedResource[]>([]),[databases,setDatabases]=useState<HostedResource[]>([]),[uncertain,setUncertain]=useState(false),[previous,setPrevious]=useState<HostedResume|null>(null);
 const interacted=useRef(false);
 const project=source?.projectName??'';
 const working=pending||localBusy;
 function restoreWork(work:HostedResume|null){if(!work)return;setPrevious(work);setMode(work.staticOnly?'static':'api');setSource(work.sourceAvailable?work.source:null);setCloudAccount(work.accountId??'');setWorkspace(work.workspaceId??'');setReceipt(work.staticReceipt??(work.provider==='cloudflare'?work.receipt??null:null));const api=work.apiReceipt??(work.provider==='render'?work.receipt??null:null);setApiReceipt(api);if(api?.serviceId)setServiceId(api.serviceId);if(api?.postgresId)setPostgresId(api.postgresId);}
 useEffect(()=>{if(!hostedClient.native())return;let live=true;void hostedClient.resume().then(work=>{if(live&&!interacted.current)restoreWork(work);}).catch(()=>{if(live)setError('이전 배포 기록을 열지 못했습니다. 새 배포 전에 제공자의 실제 상태를 확인하세요.');});return()=>{live=false;};},[]);
 async function resumeStatus(){setPending(true);setError('');try{restoreWork(await hostedClient.resumeRemote());}catch(e){setError(`이전 배포의 상태를 확인하지 못했습니다. 새로 만들지 않았습니다. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}}
 function acceptReceipt(value:HostedReceipt){
  if(value.provider==='cloudflare')setReceipt(value);else setApiReceipt(value);
  setPrevious(prior=>{
   if(!prior||!source||prior.source.publicationId!==source.publicationId||prior.source.projectName!==project)return prior;
   return {...prior,phase:value.state==='uncertain'?'uncertain':'observed',...(value.provider==='cloudflare'?{accountId:cloudAccount,staticReceipt:value}:{workspaceId:workspace,apiReceipt:value}),...(prior.provider===value.provider?{receipt:value}:{})};
  });
 }
 function invalidate(){interacted.current=true;setReceipt(null);setApiReceipt(null);setError('');setPublicConsent(false);setCostConsent(false);}
 function invalidateApi(){interacted.current=true;setApiReceipt(null);setError('');setCostConsent(false);setRecoveryConnection('');}
 async function choose(){setError('');try{const path=await hostedClient.source();if(path){setSource(path);invalidate();}}catch(e){setError(`전시 묶음을 선택하지 못했습니다. 확인 코드: ${hostedFailure(e)}`);}}
 async function deploy(){
  if(!native||pending||(mode==='api'&&uncertain)||!cloudAccount||!source||!publicConsent||!project.trim()||(mode==='api'&&(!workspace||!costConsent)))return;
  setPending(true);setError('');
  try{
   const pages=(staticOnly:boolean)=>hostedClient.operation({provider:'cloudflare',action:'deploy-static',accountId:cloudAccount,projectName:project,publicUploadApproved:publicConsent,apiRequested:mode==='api',staticOnly});
   if(mode==='static'){acceptReceipt(await pages(true));return;}
   let page=receipt;
   if(!page||page.state!=='ready'){page=await pages(true);acceptReceipt(page);if(page.state!=='ready')return;}
   let api=apiReceipt;
   if(api?.state!=='deployed'){api=await hostedClient.operation({provider:'render',action:'deploy-api',workspaceId:workspace,projectName:project,publicUploadApproved:publicConsent,costReviewed:costConsent,apiRequested:true,...(serviceId?{serviceId}:{}),...(postgresId?{postgresId}:{}),staticOnly:false});acceptReceipt(api);if(api.serviceId)setServiceId(api.serviceId);if(api.postgresId)setPostgresId(api.postgresId);if(api.state!=='deployed')return;}
   if(!api.url){setError('API의 실제 공개 주소가 아직 확인되지 않았습니다. 정적 전시는 그대로 공개돼 있습니다.');return;}
   acceptReceipt(await pages(false));
  }catch(e){if(hostedFailure(e)==='HOSTED_EXISTING_DATABASE_REQUIRED'){setError('기존 API 서버를 선택했다면 그 서버가 사용하는 데이터베이스도 목록에서 선택하세요. 새 데이터베이스를 만들지 않았습니다.');return;}if(mode==='api'&&hostedUncertain(e)){setUncertain(true);setCostConsent(false);}setError(`후속 배포 결과를 확인하지 못했습니다. 기존 정적 전시와 자료는 보존합니다. 제공자의 실제 상태를 확인하세요. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}
 }
 async function reconnectExisting(){
  if(!workspace||!serviceId||!postgresId||!recoveryProject.trim()||working)return;
  setPending(true);setError('');setRecoveryConnection('');
  try{const value=await hostedClient.operation({provider:'render',action:'connect-existing',workspaceId:workspace,serviceId,postgresId,projectName:recoveryProject.trim()});setRecoveryConnection(`백업 대상 연결을 저장했습니다. API ${value.serviceId} · DB ${value.postgresId}. 서버와 데이터베이스는 변경하지 않았습니다.`);setRecoveryGeneration(v=>v+1);}
  catch(e){setError(`기존 백업 대상을 연결하지 못했습니다. 이전 전시 기록은 보존합니다. 원래 프로젝트와 이 Mac의 기존 설정을 확인하세요. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}
 }
 async function resources(){if(!workspace)return;setPending(true);setError('');try{const value=await hostedClient.operation({provider:'render',action:'services',workspaceId:workspace});setServices(value.services??[]);setDatabases(value.postgres??[]);setUncertain(false);setCostConsent(false);}catch(e){setError(`사용 가능한 서비스를 확인하지 못했습니다. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}}
 async function refreshApi(){if(!workspace||!apiReceipt)return;setPending(true);setError('');try{acceptReceipt(await hostedClient.operation({provider:'render',action:'status',workspaceId:workspace,projectName:project,...(apiReceipt.serviceId?{serviceId:apiReceipt.serviceId}:{}),...(apiReceipt.postgresId?{postgresId:apiReceipt.postgresId}:{})}));}catch(e){setError(`API 상태를 확인하지 못했습니다. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}}
 async function refresh(){if(!receipt?.deploymentId)return;setPending(true);setError('');try{acceptReceipt(await hostedClient.operation({provider:'cloudflare',action:'status',accountId:cloudAccount,projectName:project.trim(),deploymentId:receipt.deploymentId}));}catch(e){setError(`상태를 확인하지 못했습니다. 확인 코드: ${hostedFailure(e)}`);}finally{setPending(false);}}
 return <section className="panel hosted-deployment" aria-labelledby="hosted-title"><h2 id="hosted-title">전시를 웹에 공개하기</h2><p>승인한 전시 묶음을 선택하고, 본인 계정에 배포합니다.</p>
 {previous&&<div className="next-step" role="status"><h3>이전 공개 작업</h3><p>{previous.source.title} · {previous.phase==='pending'?'완료 확인 필요':previous.phase==='uncertain'?'결과 확인 필요':'저장된 작업 기록'}</p>{!previous.sourceAvailable&&<p>전시 묶음이 없거나 변경됐습니다. 상태 조회는 가능하며, 다시 배포하려면 같은 승인 묶음을 선택하세요.</p>}<button type="button" disabled={working} onClick={()=>void resumeStatus()}>이전 배포 상태 확인</button><p>이 화면을 여는 것만으로 파일을 다시 올리거나 서버를 만들지 않습니다.</p></div>}
 {!native&&<p role="status">계정 연결과 배포는 Mac 데스크톱 앱에서 진행하세요. 이 미리보기에서는 토큰을 입력하지 않습니다.</p>}
 <fieldset disabled={!native||working}><legend>1. 필요한 공개 방식</legend><label><input type="radio" name="hosted-mode" checked={mode==='static'} onChange={()=>{setMode('static');invalidateApi();}}/>전시 보기만 · Cloudflare Pages</label><label><input type="radio" name="hosted-mode" checked={mode==='api'} onChange={()=>{setMode('api');invalidateApi();}}/>API·데이터베이스 준비 · 추가 연결 필요</label><p>{mode==='static'?'정적 전시에는 Render 계정과 데이터베이스가 필요 없습니다.':'Render의 API·PostgreSQL을 별도로 확인합니다. 요금과 무료 데이터베이스의 만료 조건을 확인하세요.'}</p></fieldset>
 <h3>2. 공개할 계정</h3><ProviderConnection provider="cloudflare" disabled={!native||working} onAccount={id=>{setCloudAccount(id);invalidate();}}/>
 {mode==='api'&&<ProviderConnection provider="render" disabled={!native||working} onAccount={id=>{setWorkspace(id);setServiceId('');setPostgresId('');setServices([]);setDatabases([]);invalidateApi();}}/>}
 <fieldset disabled={!native||working}><legend>3. 승인한 전시</legend><LocalPublicationPicker key={selectionToken??'none'} selectionToken={selectionToken} disabled={!native||pending} onBusy={setLocalBusy} onSource={value=>{setSource(value);invalidate();}}/><button type="button" onClick={()=>void choose()}>공개 승인한 전시 묶음 선택</button><p>{source?`선택한 전시: ${source.title}`:'원본 촬영이나 임의 폴더가 아닌, 공개용으로 내보낸 전시 묶음을 선택하세요.'}</p>
 <p>별도로 보관한 전시 파일을 다시 사용할 때는 보관 폴더 안의 ‘publication’ 폴더를 선택하세요. 선택만으로 공개하거나 Studio 편집본·DB를 복원하지 않습니다.</p>
 {source&&<details><summary>선택한 전시 정보</summary><p>공개 전시 ID: {source.publicationId}</p><p>예상 주소: https://{project}.pages.dev</p></details>}
 {mode==='api'&&<><button type="button" disabled={!workspace} onClick={()=>void resources()}>사용할 API·데이터베이스 확인</button><label>API 서버<select value={serviceId} onChange={e=>{setServiceId(e.target.value);invalidateApi();}}><option value="">새 무료 API 서버 만들기</option>{serviceId&&!services.some(v=>v.id===serviceId)&&<option value={serviceId}>이전 작업의 API 서버</option>}{services.map(v=><option key={v.id} value={v.id} disabled={!v.free}>{v.name}{v.free?'':' · 유료 항목'}</option>)}</select></label><label>데이터베이스<select value={postgresId} onChange={e=>{setPostgresId(e.target.value);invalidateApi();}}><option value="">새 무료 PostgreSQL 만들기</option>{postgresId&&!databases.some(v=>v.id===postgresId)&&<option value={postgresId}>이전 작업의 데이터베이스</option>}{databases.map(v=><option key={v.id} value={v.id} disabled={!v.free}>{v.name}{v.expiresAt?` · 만료 ${new Date(v.expiresAt).toLocaleDateString()}`:''}</option>)}</select></label><div role="note" aria-label="무료 데이터베이스 보존 조건"><h4>무료 DB는 생성 후 30일에 만료돼요</h4><p>카드 없이 무료 플랜으로만 요청합니다. 결제 수단 등록·유료 전환·자동 연장·자동 재생성은 하지 않습니다.</p><p>만료되면 DB에 접근할 수 없고, 14일 유예 기간 뒤 데이터가 삭제됩니다.</p><p>Render 무료 DB는 백업을 제공하지 않습니다. 만료 전에 원본과 로컬 백업을 따로 보관하세요. 이 화면은 원격 DB 백업을 자동으로 만들지 않습니다.</p></div><label><input type="checkbox" checked={costConsent} onChange={e=>setCostConsent(e.target.checked)}/>선택한 Render 서비스의 요금·데이터 보존·만료 조건을 확인했습니다.</label></>}
 {mode==='api'&&<section aria-label="기존 서버 백업 대상 연결"><h4>기존 서버를 백업 대상으로 연결</h4><p>위에서 기존 API 서버와 데이터베이스를 선택하세요. 이 Mac에 보관된 원래 서버 설정과 일치하는지 확인하고 연결만 저장합니다. 현재 전시 파일은 묶지 않으며 백업은 별도로 시작합니다.</p><label>원래 배포 프로젝트 이름<input value={recoveryProject} onChange={e=>{setRecoveryProject(e.target.value);setRecoveryConnection('');}} maxLength={63}/></label><button type="button" disabled={!workspace||!serviceId||!postgresId||!recoveryProject.trim()} onClick={()=>void reconnectExisting()}>기존 백업 대상 연결</button>{recoveryConnection&&<p role="status">{recoveryConnection}</p>}</section>}
 <label><input type="checkbox" checked={publicConsent} onChange={e=>setPublicConsent(e.target.checked)}/>이 전시의 공개 권리를 확인했으며 선택한 계정에 파일을 올리는 데 동의합니다.</label>
 <button type="button" className="primary" disabled={(mode==='api'&&uncertain)||!cloudAccount||!source||!project.trim()||!publicConsent||(mode==='api'&&(!workspace||!costConsent))} onClick={()=>void deploy()}>선택한 전시 배포</button></fieldset>
 {pending&&<div role="status"><progress aria-label="계정 제공자 작업 진행 중"/>제공자 응답을 확인하고 있습니다… 이 창을 유지하세요.</div>}
 {error&&<p role="alert">{error}</p>}{mode==='api'&&uncertain&&<p>생성 여부가 불확실해 새 배포를 멈췄습니다. 제공자의 실제 상태와 기존 서비스 목록을 먼저 확인하세요.</p>}
 {apiReceipt&&<p role="status">API 상태: {apiReceipt.state==='deployed'?'제공자 배포 완료 · 실제 API 응답은 별도 확인':apiReceipt.state==='deploying'?'준비 중':'완료 확인 필요'}{apiReceipt.expiresAt?` · 제공자가 알려 준 만료일: ${new Date(apiReceipt.expiresAt).toLocaleDateString()}`:''}<button type="button" disabled={working} onClick={()=>void refreshApi()}>API 상태 다시 확인</button></p>}
 {receipt&&<div role="status"><h3>{receipt.state==='ready'?(mode==='static'?'전시 배포 확인 완료':'정적 전시 배포 완료 · API 데이터 연결 미완료'):receipt.state==='deploying'?'전시 배포 진행 중':'배포 완료 확인 필요'}</h3>{receipt.state==='ready'&&receipt.url&&<p><button type="button" onClick={()=>void hostedClient.openUrl(receipt.url!).catch(()=>setError('공개 전시를 열지 못했습니다. 배포 상태를 다시 확인하세요.'))}>공개 전시 열기</button></p>}{receipt.deploymentId&&<button type="button" disabled={working} onClick={()=>void refresh()}>배포 상태 다시 확인</button>}<p>준비 중이거나 응답이 불확실하면 배포가 없었다고 가정하지 마세요. 제공자에서 실제 상태를 확인하세요.</p></div>}
 {mode==='api'&&<HostedRecovery key={recoveryGeneration} selectionToken={selectionToken}/>}
 </section>;
}
