// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {readFile,lstat,readdir,realpath} from 'node:fs/promises';
import {resolve,join,extname,isAbsolute} from 'node:path';
import {createHash,createPrivateKey,createPublicKey} from 'node:crypto';
import {pagesAssetHash} from './pages-asset-hash.mjs';
import {pagesDeploymentIdentity,appendPagesProxy,dynamicPublicationHTML} from './pages-render-worker.mjs';
const CF='https://api.cloudflare.com/client/v4',RENDER='https://api.render.com/v1';
const fail=code=>{throw Error(code);},sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const text=v=>typeof v==='string'&&v.length>0&&v.length<=128&&!/[\x00-\x1f\x7f]/.test(v);
const name=v=>typeof v==='string'&&/^[a-z0-9](?:[a-z0-9-]{0,57}[a-z0-9])?$/.test(v);
const id=v=>text(v)&&/^[a-zA-Z0-9_-]+$/.test(v);
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const object=v=>v&&typeof v==='object'&&!Array.isArray(v);
const safeName=v=>typeof v==='string'?v.replace(/[\x00-\x1f\x7f]/g,'').slice(0,128):'';
const baseResult=(provider,action,state)=>({provider,action,state});
function publicURL(value,domain){try{const u=new URL(value);if(u.protocol!=='https:'||u.username||u.password||u.port||u.search||u.hash||u.pathname!=='/'||!u.hostname.endsWith('.'+domain)||!/^[a-z0-9.-]+$/.test(u.hostname))return undefined;return u.origin;}catch{return undefined;}}
async function boundedResponse(response){const reader=response.body?.getReader();if(!reader)return null;const chunks=[];let size=0;try{while(true){const {done,value}=await reader.read();if(done)break;size+=value.length;if(size>4*1024*1024)fail('HOSTED_RESPONSE_INVALID');chunks.push(value);}}finally{await reader.cancel().catch(()=>{});}try{return JSON.parse(Buffer.concat(chunks).toString());}catch{fail('HOSTED_RESPONSE_INVALID');}}
function cfStage(path,method){
 if(path.startsWith('/pages/assets/check-missing'))return 'ASSET_CHECK';
 if(path.startsWith('/pages/assets/upsert-hashes'))return 'ASSET_UPSERT';
 if(path.startsWith('/pages/assets/upload'))return 'ASSET_UPLOAD';
 if(path.endsWith('/upload-token'))return 'UPLOAD_TOKEN';
 if(/\/deployments\//.test(path))return 'DEPLOY_STATUS';
 if(/\/deployments(?:\?|$)/.test(path))return method==='POST'?'DEPLOY_CREATE':'DEPLOY_LIST';
 if(/\/pages\/projects$/.test(path))return 'PROJECT_CREATE';
 if(/\/pages\/projects\//.test(path))return 'PROJECT_READ';
 return null;
}
function cfDiagnostic(path,method,status,value){
 const stage=cfStage(path,method);if(!stage)return null;
 // Allow only fixed stages, bounded HTTP integers and one numeric provider code.
 // No remote message, URL, request path, header or account identifier is retained.
 const code=Array.isArray(value?.errors)?value.errors.find(e=>Number.isSafeInteger(e?.code)&&e.code>=1000&&e.code<=99999999)?.code:undefined;
 return `HOSTED_CF_${stage}_HTTP_${status}${code===undefined?'':'_CODE_'+code}`;
}
const HOSTED_STAGES=new Set(['ACCOUNTS','MEMBERSHIPS','PROJECT_READ','PROJECT_CREATE','UPLOAD_TOKEN','ASSET_CHECK','ASSET_UPLOAD','ASSET_UPSERT','DEPLOY_LIST','DEPLOY_STATUS','DEPLOY_CREATE','BUNDLE_VERIFY','ASSET_PREPARE','DEPLOY_FORM','RENDER_REQUEST']);
function stage(options,value){if(HOSTED_STAGES.has(value)&&typeof options?.onStage==='function')options.onStage(value);}
function transport(provider,{fetch:fetcher=globalThis.fetch,tokens={},onStage}={}){
 const token=tokens[provider];if(typeof token!=='string'||!token.length||token.length>8192||/[\r\n\0]/.test(token))fail('HOSTED_TOKEN_REQUIRED');
 return async(path,{method='GET',body,bearer=token,missing=false,pageInfo=false}={})=>{
  stage({onStage},provider==='cloudflare'?(cfStage(path,method)??(path.startsWith('/memberships?')?'MEMBERSHIPS':'ACCOUNTS')):'RENDER_REQUEST');
  let response;try{response=await fetcher((provider==='cloudflare'?CF:RENDER)+path,{method,redirect:'error',headers:{Authorization:'Bearer '+bearer,Accept:'application/json',...(body!==undefined&&!(body instanceof FormData)?{'Content-Type':'application/json'}:{})},...(body===undefined?{}:{body:body instanceof FormData?body:JSON.stringify(body)}),signal:AbortSignal.timeout(90000)});}catch{fail(method==='GET'?'HOSTED_CONNECTION_FAILED':'HOSTED_RESULT_UNCERTAIN');}
  if(response.redirected||response.status>=300&&response.status<400)fail(method==='GET'?'HOSTED_REDIRECT_REFUSED':'HOSTED_RESULT_UNCERTAIN');
  if(missing&&response.status===404){await response.body?.cancel().catch(()=>{});return null;}
  if(!response.ok){let diagnostic;if(provider==='cloudflare'&&response.status>=400&&response.status<500&&!([402,429].includes(response.status))){let value;try{value=await boundedResponse(response);}catch{}diagnostic=cfDiagnostic(path,method,response.status,value);}else await response.body?.cancel().catch(()=>{});if(diagnostic)fail(diagnostic);if(response.status===401)fail('HOSTED_TOKEN_INVALID');if(response.status===403)fail('HOSTED_PERMISSION_REQUIRED');if(response.status===402)fail('HOSTED_PAID_PLAN_REFUSED');if(response.status===429)fail('HOSTED_RATE_LIMITED');fail(method!=='GET'&&response.status>=500?'HOSTED_RESULT_UNCERTAIN':'HOSTED_REMOTE_REJECTED');}
  let value;try{value=await boundedResponse(response);}catch{fail(method==='GET'?'HOSTED_RESPONSE_INVALID':'HOSTED_RESULT_UNCERTAIN');}if(provider==='cloudflare'){if(value?.success!==true)fail(cfDiagnostic(path,method,response.status,value)??'HOSTED_REMOTE_REJECTED');return pageInfo?{result:value.result,totalPages:Number.isSafeInteger(value.result_info?.total_pages)&&value.result_info.total_pages>=0&&value.result_info.total_pages<=100000?value.result_info.total_pages:undefined}:value.result;}return value;
 };
}
const MIME={'.html':'text/html','.js':'application/javascript','.mjs':'application/javascript','.css':'text/css','.json':'application/json','.svg':'image/svg+xml','.png':'image/png','.jpg':'image/jpeg','.jpeg':'image/jpeg','.webp':'image/webp','.ico':'image/x-icon','.woff':'font/woff','.woff2':'font/woff2','.glb':'model/gltf-binary','.bin':'application/octet-stream','.txt':'text/plain'};
async function regular(path,max){const s=await lstat(path);if(!s.isFile()||s.isSymbolicLink()||s.nlink!==1||s.size>max)fail('HOSTED_STATIC_BUNDLE_INVALID');const bytes=await readFile(path);const after=await lstat(path);if(after.ino!==s.ino||after.dev!==s.dev||after.size!==bytes.length||after.mtimeMs!==s.mtimeMs)fail('HOSTED_STATIC_BUNDLE_CHANGED');return bytes;}
export async function inspectStaticBundle(sourcePath,{expectedManifestSha256}={}){
 try{
 if(typeof sourcePath!=='string'||!isAbsolute(sourcePath))fail('HOSTED_STATIC_BUNDLE_INVALID');const root=await realpath(sourcePath);if((await lstat(sourcePath)).isSymbolicLink()||!(await lstat(root)).isDirectory())fail('HOSTED_STATIC_BUNDLE_INVALID');
 const receiptBytes=await regular(join(root,'static-export-manifest.json'),8*1024*1024);if(expectedManifestSha256!==undefined&&(!hex(expectedManifestSha256)||sha(receiptBytes)!==expectedManifestSha256))fail('HOSTED_STATIC_BUNDLE_CHANGED');const receipt=JSON.parse(receiptBytes);
 if(receipt.format!=='exhibitos-static-publication-v1'||!text(receipt.publicationId)||!hex(receipt.revisionSha256)||!hex(receipt.indexSha256)||!Array.isArray(receipt.files)||receipt.files.length<2||receipt.files.length>=20000)fail('HOSTED_STATIC_BUNDLE_INVALID');
 const files=[],seen=new Set();let total=0;
 for(const file of receipt.files){if(!object(file)||typeof file.path!=='string'||file.path.length>1024||file.path.split('/').some(s=>!s||s==='.'||s==='..'||s.startsWith('.'))||/[\\\x00-\x1f\x7f]/.test(file.path)||seen.has(file.path)||file.path==='static-export-manifest.json'||!Number.isSafeInteger(file.bytes)||file.bytes<0||file.bytes>25*1024*1024||!hex(file.sha256)||file.path.split('/').some(s=>['functions','_worker.js','_worker.bundle','_routes.json','_headers','_redirects'].includes(s)))fail('HOSTED_STATIC_BUNDLE_INVALID');seen.add(file.path);const bytes=await regular(join(root,file.path),25*1024*1024);if(bytes.length!==file.bytes||sha(bytes)!==file.sha256)fail('HOSTED_STATIC_BUNDLE_CHANGED');total+=bytes.length;files.push({path:file.path,bytes:file.bytes,sha256:file.sha256,mime:MIME[extname(file.path)]??'application/octet-stream'});}
 const actual=[];async function walk(dir,prefix=''){for(const entry of await readdir(dir,{withFileTypes:true})){if(entry.isSymbolicLink())fail('HOSTED_STATIC_BUNDLE_INVALID');const path=prefix+entry.name;if(entry.isDirectory())await walk(join(dir,entry.name),path+'/');else if(entry.isFile())actual.push(path);else fail('HOSTED_STATIC_BUNDLE_INVALID');}}await walk(root);
 if(actual.length!==files.length+1||actual.some(p=>p!=='static-export-manifest.json'&&!seen.has(p)))fail('HOSTED_STATIC_BUNDLE_CHANGED');
 const index=await regular(join(root,'static-publication-index.json'),8*1024*1024);if(sha(index)!==receipt.indexSha256)fail('HOSTED_STATIC_BUNDLE_CHANGED');const html=(await regular(join(root,'index.html'),25*1024*1024)).toString();if(!html.includes(receipt.indexSha256)||!html.includes('exhibitos-static-index'))fail('HOSTED_STATIC_BUNDLE_INVALID');
 const projection=JSON.parse(index);if(projection.version!==1||!Array.isArray(projection.assets)||!object(projection.publication))fail('HOSTED_STATIC_BUNDLE_INVALID');
 for(const d of [projection.publication,...projection.assets])if(!files.some(f=>f.path===d.path&&f.bytes===d.bytes&&f.sha256===d.sha256))fail('HOSTED_STATIC_BUNDLE_INVALID');
 const publication=JSON.parse((await regular(join(root,projection.publication.path),25*1024*1024)).toString());if(publication.publication?.id!==receipt.publicationId||publication.publication?.status!=='published'||publication.publication?.revisionSha256!==receipt.revisionSha256)fail('HOSTED_STATIC_BUNDLE_INVALID');
 // The private local upload receipt itself is not a published asset.
 return {root,files,bytes:total,manifestSha256:sha(receiptBytes),publicationId:receipt.publicationId,revisionSha256:receipt.revisionSha256};
 }catch(e){if(/^HOSTED_/.test(e.message))throw e;fail('HOSTED_STATIC_BUNDLE_INVALID');}
}
async function cfProject(api,accountId,projectName){const route=`/accounts/${accountId}/pages/projects/${projectName}`;let p=await api(route,{missing:true});if(!p){try{p=await api(`/accounts/${accountId}/pages/projects`,{method:'POST',body:{name:projectName,production_branch:'main'}});}catch(e){if(e.message!=='HOSTED_RESULT_UNCERTAIN')throw e;p=await api(route,{missing:true});if(!p)fail('HOSTED_RESULT_UNCERTAIN');}}if(p?.name!==projectName||p.source||(p.production_branch&&p.production_branch!=='main'))fail('HOSTED_PROJECT_CONFLICT');if(p.subdomain&&p.subdomain!==projectName+'.pages.dev')fail('HOSTED_CANONICAL_ORIGIN_CONFLICT');return p;}
function deploymentProjection(d,projectName,action){if(!id(d?.id))fail('HOSTED_RESPONSE_INVALID');if(d.environment==='preview'||(d.deployment_trigger?.metadata?.branch&&d.deployment_trigger.metadata.branch!=='main'))fail('HOSTED_DEPLOYMENT_BRANCH_CONFLICT');const status=d.latest_stage?.status,state=status==='success'?'ready':['failure','canceled'].includes(status)?'failed':'deploying';const canonical='https://'+projectName+'.pages.dev',provided=publicURL(d.url,'pages.dev');const linked=provided&&(provided===canonical||new URL(provided).hostname.endsWith('.'+projectName+'.pages.dev'))||Array.isArray(d.aliases)&&d.aliases.some(a=>publicURL(a,'pages.dev')===canonical);return {...baseResult('cloudflare',action,state),projectName,deploymentId:d.id,...(linked?{url:canonical}:{})};}
// Official Project.canonical_deployment identifies the most recent production deployment:
// https://developers.cloudflare.com/api/resources/pages/subresources/projects/methods/get/
// A historical deployment success alone does not prove the canonical URL serves it.
async function observeDeployment(api,route,deployment,projectName,action){
 const result=deploymentProjection(deployment,projectName,action);
 if(result.state!=='ready')return result;
 const project=await api(route);if(project?.name!==projectName)fail('HOSTED_PROJECT_CONFLICT');
 if(project.canonical_deployment?.id!==deployment.id){result.state='uncertain';delete result.url;}
 return result;
}
function currentContent(project,hash){return project.canonical_deployment?.deployment_trigger?.metadata?.commit_hash===hash&&project.canonical_deployment?.latest_stage?.status==='success'?project.canonical_deployment:null;}
async function findDeployment(api,route,hash){
 for(let page=1;page<=100;page++){
  const {result:list,totalPages}=await api(route+`?per_page=20&page=${page}`,{pageInfo:true});
  if(!Array.isArray(list)||list.length>20)fail('HOSTED_RESPONSE_INVALID');
  const found=list.find(d=>d.deployment_trigger?.metadata?.commit_hash===hash);if(found)return found;
  if(totalPages!==undefined?page>=totalPages:list.length<20)return null;
 }
 fail('HOSTED_PAGINATION_LIMIT');
}
async function cloudflare(request,options){const api=transport('cloudflare',options),action=request.action;
 if(action==='accounts'){
  const accounts=new Map();let fallback=false,complete=false;
  try{for(let page=1;page<=100;page++){const rows=await api(`/accounts?per_page=50&page=${page}`);if(!Array.isArray(rows))fail('HOSTED_RESPONSE_INVALID');for(const a of rows){if(!id(a.id))fail('HOSTED_RESPONSE_INVALID');accounts.set(a.id,{id:a.id,name:safeName(a.name)});}if(rows.length<50){complete=true;break;}}if(!complete)fail('HOSTED_PAGINATION_LIMIT');}
  catch(e){if(e.message!=='HOSTED_PERMISSION_REQUIRED')throw e;fallback=true;}
  if(fallback||!accounts.size){complete=false;for(let page=1;page<=100;page++){const rows=await api(`/memberships?status=accepted&per_page=50&page=${page}`);if(!Array.isArray(rows))fail('HOSTED_RESPONSE_INVALID');for(const membership of rows){if(membership?.status!=='accepted'||membership.api_access_enabled===false)continue;const a=membership.account;if(!object(a)||!(/^[a-f0-9]{32}$/i.test(a.id)))fail('HOSTED_RESPONSE_INVALID');accounts.set(a.id,{id:a.id,name:safeName(a.name)});}if(rows.length<50){complete=true;break;}}if(!complete)fail('HOSTED_PAGINATION_LIMIT');}
  return {...baseResult('cloudflare',action,'ready'),accounts:[...accounts.values()]};
 }
 if(!/^[a-f0-9]{32}$/.test(request.accountId)||!name(request.projectName))fail('HOSTED_REQUEST_INVALID');const route=`/accounts/${request.accountId}/pages/projects/${request.projectName}`,deployments=route+'/deployments';
 if(action==='reconcile'){if(!hex(request.expectedManifestSha256))fail('HOSTED_REQUEST_INVALID');const project=await api(route);if(project?.name!==request.projectName)fail('HOSTED_PROJECT_CONFLICT');const {hash}=pagesDeploymentIdentity(request.expectedManifestSha256,request),d=await findDeployment(api,deployments,hash);return d?observeDeployment(api,route,d,request.projectName,action):{...baseResult('cloudflare',action,'uncertain'),projectName:request.projectName,deploymentId:'pending-'+hash};}
 if(action==='status'){if(/^pending-[a-f0-9]{40}$/.test(request.deploymentId)){const d=await findDeployment(api,deployments,request.deploymentId.slice(8));return d?observeDeployment(api,route,d,request.projectName,action):{...baseResult('cloudflare',action,'uncertain'),projectName:request.projectName,deploymentId:request.deploymentId};}if(!id(request.deploymentId))fail('HOSTED_REQUEST_INVALID');return observeDeployment(api,route,await api(deployments+'/'+request.deploymentId),request.projectName,action);}
 if(action!=='deploy-static')fail('HOSTED_REQUEST_INVALID');stage(options,'BUNDLE_VERIFY');const bundle=await inspectStaticBundle(request.sourcePath,{expectedManifestSha256:request.expectedManifestSha256});const {hash:contentHash,proxy}=pagesDeploymentIdentity(bundle.manifestSha256,request);const project=await cfProject(api,request.accountId,request.projectName),current=currentContent(project,contentHash);if(current)return deploymentProjection(current,request.projectName,action);
 const previous=await findDeployment(api,deployments,contentHash);
 if(previous){const observed=await observeDeployment(api,route,previous,request.projectName,action);if(['ready','deploying'].includes(observed.state))return observed;}
 // An explicit deploy retries terminal failure or republishes historical success through
 // the existing exact Direct Upload manifest, branch=main, including the selected Worker mode.
 // https://developers.cloudflare.com/api/resources/pages/subresources/projects/subresources/deployments/methods/create/

 const jwt=(await api(route+'/upload-token'))?.jwt;if(typeof jwt!=='string'||!jwt.length||jwt.length>16384)fail('HOSTED_RESPONSE_INVALID');
 stage(options,'ASSET_PREPARE');const manifest={},hashFiles=new Map();for(const file of bundle.files){const bytes=await regular(join(bundle.root,file.path),25*1024*1024);if(sha(bytes)!==file.sha256)fail('HOSTED_STATIC_BUNDLE_CHANGED');const uploadBytes=proxy&&file.path==='index.html'?dynamicPublicationHTML(bytes):bytes;const hash=pagesAssetHash(uploadBytes,file.path);manifest['/'+file.path]=hash;if(!hashFiles.has(hash))hashFiles.set(hash,{...file,...(proxy&&file.path==='index.html'?{generatedBytes:uploadBytes}:{})});}
 const hashes=[...hashFiles.keys()],missing=await api('/pages/assets/check-missing',{method:'POST',bearer:jwt,body:{hashes}});if(!Array.isArray(missing)||missing.some(h=>!hashFiles.has(h)))fail('HOSTED_RESPONSE_INVALID');
 for(const hash of new Set(missing)){const f=hashFiles.get(hash),bytes=await regular(join(bundle.root,f.path),25*1024*1024);if(sha(bytes)!==f.sha256)fail('HOSTED_STATIC_BUNDLE_CHANGED');await api('/pages/assets/upload',{method:'POST',bearer:jwt,body:[{key:hash,value:(f.generatedBytes??bytes).toString('base64'),metadata:{contentType:f.mime},base64:true}]});}
 await api('/pages/assets/upsert-hashes',{method:'POST',bearer:jwt,body:{hashes}});
 stage(options,'DEPLOY_FORM');const form=new FormData();form.set('manifest',JSON.stringify(manifest));form.set('branch','main');form.set('commit_hash',contentHash);form.set('commit_message','ExhibitOS approved static publication');if(proxy)await appendPagesProxy(form,proxy);
 let created;
 try{created=await api(deployments,{method:'POST',body:form});}catch(e){
  if(e.message!=='HOSTED_RESULT_UNCERTAIN')throw e;
  try{const observedProject=await api(route),active=currentContent(observedProject,contentHash);if(observedProject?.name!==request.projectName)fail('HOSTED_PROJECT_CONFLICT');if(active)return deploymentProjection(active,request.projectName,action);const d=await findDeployment(api,deployments,contentHash);if(d&&d.id!==previous?.id)return await observeDeployment(api,route,d,request.projectName,action);}catch{}
  return {...baseResult('cloudflare',action,'uncertain'),projectName:request.projectName,deploymentId:'pending-'+contentHash};
 }
 return observeDeployment(api,route,created,request.projectName,action);
}
async function renderList(api,kind,query=''){const all=[];let cursor;for(let page=0;page<100;page++){const data=await api(`/${kind}?limit=100${query}${cursor?'&cursor='+encodeURIComponent(cursor):''}`);if(!Array.isArray(data))fail('HOSTED_RESPONSE_INVALID');for(const item of data){const resource=item[kind==='owners'?'owner':kind==='services'?'service':'postgres'];if(!object(resource))fail('HOSTED_RESPONSE_INVALID');all.push(resource);}if(data.length<100)return all;const next=data.at(-1)?.cursor;if(!text(next)||next===cursor)fail('HOSTED_RESPONSE_INVALID');cursor=next;}fail('HOSTED_PAGINATION_LIMIT');}
function renderIdentity(request){if(!id(request.workspaceId)||!name(request.projectName))fail('HOSTED_REQUEST_INVALID');return {ownerId:request.workspaceId,name:request.projectName};}
async function uniqueResource(api,kind,ownerId,name){const matches=(await renderList(api,kind,'&ownerId='+encodeURIComponent(ownerId)+'&name='+encodeURIComponent(name))).filter(r=>(r.ownerId??r.owner?.id)===ownerId&&r.name===name);if(matches.length>1)fail('HOSTED_RESOURCE_CONFLICT');return matches[0]??null;}
// Provider timestamps are RFC3339 date-times, not Date.parse's legacy date syntax.
// Reject calendar rollover too: an invalid provider value cannot prove a free slot.
function postgresExpiry(value){
 if(value===undefined)return undefined;
 const m=typeof value==='string'&&/^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})$/.exec(value);
 if(!m)fail('HOSTED_RESPONSE_INVALID');
 const [year,month,day,hour,minute,second]=m.slice(1).map(Number),leap=year%4===0&&(year%100!==0||year%400===0),days=[31,leap?29:28,31,30,31,30,31,31,30,31,30,31];
 if(month<1||month>12||day<1||day>days[month-1]||hour>23||minute>59||second>59)fail('HOSTED_RESPONSE_INVALID');
 const instant=Date.parse(value);if(!Number.isFinite(instant))fail('HOSTED_RESPONSE_INVALID');return instant;
}
function projectDatabase(db){const instant=postgresExpiry(db.expiresAt),expiry=instant===undefined?undefined:new Date(instant).toISOString();return {id:db.id,name:safeName(db.name),plan:'free',status:db.status==='available'?'available':db.status==='creating'?'creating':'unavailable',...(expiry?{expiresAt:expiry}:{})};}
function projectService(service){const url=publicURL(service.serviceDetails?.url,'onrender.com');return {id:service.id,name:safeName(service.name),plan:'free',...(url?{url}:{})};}
function assertFree(resource,kind){if(!id(resource?.id)||(kind==='services'?resource.serviceDetails?.plan:resource.plan)!=='free')fail('HOSTED_PAID_PLAN_REFUSED');return resource;}
// Selected/reconciled resources must meet the same contract as newly requested ones.
function admittedFreePostgres(db,identity,{selectedId,expectedName}={}){
 assertFree(db,'postgres');
 if(db.owner?.id!==identity.ownerId||db.region!=='frankfurt'||db.version!=='18'||selectedId&&db.id!==selectedId||expectedName&&db.name!==expectedName)fail('HOSTED_RESOURCE_CONFLICT');
 const expires=postgresExpiry(db.expiresAt);
 if(expires!==undefined&&expires<=Date.now())fail('HOSTED_RECOVERY_DATABASE_EXPIRED');
 if(!['creating','available'].includes(db.status))fail('HOSTED_RESOURCE_CONFLICT');
 return projectDatabase(db);
}
export async function createFreePostgres(request,options){
 if(options?.onPostgresCreated!==undefined&&typeof options.onPostgresCreated!=='function')fail('HOSTED_REQUEST_INVALID');
 const api=transport('render',options),identity=renderIdentity(request),dbName=identity.name+'-db';
 if(request.postgresId){if(!id(request.postgresId))fail('HOSTED_REQUEST_INVALID');return admittedFreePostgres(await api('/postgres/'+request.postgresId),identity,{selectedId:request.postgresId});}
 let db=await uniqueResource(api,'postgres',identity.ownerId,dbName);
 if(db)return admittedFreePostgres(db,identity,{expectedName:dbName});
 // Render allows one active free PostgreSQL instance per workspace. Never POST
 // another candidate when a free instance is active/uncertain; no deletion/upgrade.
 // Re-read exact operation name in the full list to preserve lost-response reuse.
 const listed=await renderList(api,'postgres','&ownerId='+encodeURIComponent(identity.ownerId));
 if(listed.some(r=>!id(r.id)||!id(r.owner?.id)||typeof r.plan!=='string'||!r.plan))fail('HOSTED_RESPONSE_INVALID');
 const owned=listed.filter(r=>r.owner.id===identity.ownerId);
 const same=owned.filter(r=>r.name===dbName);
 if(same.length>1)fail('HOSTED_RESOURCE_CONFLICT');
 if(same.length===1)return admittedFreePostgres(same[0],identity,{expectedName:dbName});
 // A valid provider expiry is the only inactivity proof. A suspended or
 // unavailable status alone never releases a slot, nor does a missing expiry.
 const knownStatus=new Set(['creating','available','unavailable','config_restart','suspended','maintenance_scheduled','maintenance_in_progress','recovery_failed','recovery_in_progress','updating_instance']);
 for(const retained of owned.filter(r=>r.plan==='free')){
  if(!knownStatus.has(retained.status))fail('HOSTED_RESOURCE_CONFLICT');
  const expires=postgresExpiry(retained.expiresAt);
  if(expires===undefined||expires>Date.now())fail('HOSTED_RESOURCE_CONFLICT');
 }

 let created=false;
 try{db=await api('/postgres',{method:'POST',body:{name:dbName,ownerId:identity.ownerId,plan:'free',version:'18',region:'frankfurt',enableHighAvailability:false,enableDiskAutoscaling:false,ipAllowList:[]}});created=true;}
 catch(e){if(e.message!=='HOSTED_RESULT_UNCERTAIN')throw e;db=await uniqueResource(api,'postgres',identity.ownerId,dbName);if(!db)fail('HOSTED_RESULT_UNCERTAIN');}
 const candidate=admittedFreePostgres(db,identity,{expectedName:dbName});
 // Only the actual successful POST response establishes this acquisition event.
 // Same-name discovery or a lost-response reconciliation never grants it.
 if(created&&options?.onPostgresCreated){try{await options.onPostgresCreated({id:db.id,ownerId:identity.ownerId,name:dbName});}catch{fail('HOSTED_RESULT_UNCERTAIN');}}
 return candidate;
}

const REQUIRED_ENV=['CLIENT_ORIGIN','TENANT_ID','ADMIN_SUBJECT','ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'];
function runtimeEnv(options,{database=false}={}){const env=options.runtime?.envVars;if(!Array.isArray(env)||![...REQUIRED_ENV,...(database?['DATABASE_URL']:[])].every(k=>env.some(e=>e.key===k&&typeof e.value==='string'&&e.value.length>0))||env.some(e=>!object(e)||typeof e.key!=='string'||!(/^[A-Z][A-Z0-9_]{0,63}$/.test(e.key))||typeof e.value!=='string'||e.value.length>32768||/[\0]/.test(e.value))||new Set(env.map(e=>e.key)).size!==env.length)fail('RENDER_CONFIGURATION_REQUIRED');if(!publicURL(env.find(e=>e.key==='CLIENT_ORIGIN').value,'pages.dev'))fail('RENDER_CONFIGURATION_REQUIRED');return env;}

export async function createFreeService(request,options){const api=transport('render',options),identity=renderIdentity(request),envVars=runtimeEnv(options,{database:true});
 let service;if(request.serviceId){if(!id(request.serviceId))fail('HOSTED_REQUEST_INVALID');service=await api('/services/'+request.serviceId);}else service=await uniqueResource(api,'services',identity.ownerId,identity.name);if(service){assertFree(service,'services');if(service.ownerId!==identity.ownerId||service.type!=='web_service'||service.repo!=='https://github.com/ExhibitOS/platform'||service.serviceDetails?.runtime!=='node'||service.serviceDetails?.healthCheckPath!=='/api/v1/readiness')fail('HOSTED_RESOURCE_CONFLICT');const saved=await api('/services/'+service.id+'/env-vars?limit=100');if(!Array.isArray(saved)||!envVars.every(e=>saved.some(x=>x.envVar?.key===e.key&&x.envVar?.value===e.value)))fail('HOSTED_RESOURCE_CONFLICT');return projectService(service);}
 try{const result=await api('/services',{method:'POST',body:{type:'web_service',name:identity.name,ownerId:identity.ownerId,repo:'https://github.com/ExhibitOS/platform',branch:'main',autoDeployTrigger:'off',envVars,serviceDetails:{runtime:'node',plan:'free',region:'frankfurt',numInstances:1,healthCheckPath:'/api/v1/readiness',envSpecificDetails:{buildCommand:'npm ci && npm run build',startCommand:'node scripts/render-runtime.mjs'}}}});service=result.service??result;}catch(e){if(e.message!=='HOSTED_RESULT_UNCERTAIN')throw e;service=await uniqueResource(api,'services',identity.ownerId,identity.name);if(!service)fail('HOSTED_RESULT_UNCERTAIN');}assertFree(service,'services');if(service.name!==identity.name||service.ownerId!==identity.ownerId)fail('HOSTED_RESOURCE_CONFLICT');return projectService(service);}
export async function listRenderResources(request,options){if(!id(request.workspaceId))fail('HOSTED_REQUEST_INVALID');const api=transport('render',options);const services=await renderList(api,'services','&ownerId='+request.workspaceId),postgres=await renderList(api,'postgres','&ownerId='+request.workspaceId);return {services:services.filter(r=>r.ownerId===request.workspaceId).map(r=>({id:r.id,name:safeName(r.name),free:r.serviceDetails?.plan==='free'})),postgres:postgres.filter(r=>r.owner?.id===request.workspaceId).map(r=>({id:r.id,name:safeName(r.name),free:r.plan==='free',...(postgresExpiry(r.expiresAt)!==undefined?{expiresAt:new Date(postgresExpiry(r.expiresAt)).toISOString()}:{})}))};}
export async function executeHosted(request,options={}){
 if(!object(request)||!['cloudflare','render'].includes(request.provider)||!['accounts','services','deploy-static','deploy-api','status','reconcile'].includes(request.action))fail('HOSTED_REQUEST_INVALID');
 if(request.provider==='cloudflare'){if(['deploy-api','services'].includes(request.action))fail('HOSTED_REQUEST_INVALID');return cloudflare(request,options);}
 if(request.staticOnly===true)fail('HOSTED_RENDER_NOT_REQUIRED');const api=transport('render',options),action=request.action;
 if(action==='accounts'){const owners=await renderList(api,'owners');return {...baseResult('render',action,'ready'),accounts:owners.map(r=>{if(!id(r.id))fail('HOSTED_RESPONSE_INVALID');return {id:r.id,name:safeName(r.name)};})};}
 if(action==='reconcile'){
  const identity=renderIdentity(request);let serviceId=request.serviceId,postgresId=request.postgresId;
  if(!serviceId){const service=await uniqueResource(api,'services',identity.ownerId,identity.name);if(service){assertFree(service,'services');serviceId=service.id;}}
  if(!postgresId){const db=await uniqueResource(api,'postgres',identity.ownerId,identity.name+'-db');if(db){assertFree(db,'postgres');postgresId=db.id;}}
  if(!serviceId&&!postgresId)return {...baseResult('render',action,'uncertain')};
  const observed=await executeHosted({...request,action:'status',serviceId,postgresId},options);observed.action=action;
  if(!serviceId)observed.state=observed.databaseState==='creating'?'deploying':'needs-configuration';
  if(serviceId&&!postgresId)observed.state='uncertain';
  return observed;
 }
 if(action==='services')return {...baseResult('render',action,'ready'),...await listRenderResources(request,options)};
 if(action==='deploy-api'){
  if(request.serviceId&&!request.postgresId)fail('RENDER_EXISTING_DATABASE_REQUIRED');
  const envVars=runtimeEnv(options),db=await createFreePostgres(request,options);const expiresAt=projectDatabase(db).expiresAt;
  const result={...baseResult('render',action,'deploying'),postgresId:db.id,...(expiresAt?{expiresAt}:{})};
  if(expiresAt&&Date.parse(expiresAt)<=Date.now())return {...result,state:'failed'};
  if(db.status!=='available')return result;
  const info=await api('/postgres/'+db.id+'/connection-info');if(typeof info?.internalConnectionString!=='string'||!/^postgres(?:ql)?:\/\//.test(info.internalConnectionString))fail('HOSTED_RESPONSE_INVALID');
  const runtime={envVars:[...envVars.filter(e=>e.key!=='DATABASE_URL'),{key:'DATABASE_URL',value:info.internalConnectionString}]};
  const service=await createFreeService(request,{...options,runtime});return {...result,serviceId:service.id,...(service.url?{url:service.url}:{})};
 }

 if(action!=='status'||!id(request.workspaceId))fail('HOSTED_REQUEST_INVALID');const result={...baseResult('render',action,'needs-configuration')};
 if(request.postgresId){if(!id(request.postgresId))fail('HOSTED_REQUEST_INVALID');const db=assertFree(await api('/postgres/'+request.postgresId),'postgres');if(db.owner?.id!==request.workspaceId)fail('HOSTED_RESOURCE_CONFLICT');result.postgresId=db.id;if(postgresExpiry(db.expiresAt)!==undefined)result.expiresAt=new Date(postgresExpiry(db.expiresAt)).toISOString();result.databaseState=db.status==='available'?'available':db.status==='creating'?'creating':'unavailable';}
 if(request.serviceId){if(!id(request.serviceId))fail('HOSTED_REQUEST_INVALID');const service=assertFree(await api('/services/'+request.serviceId),'services');if(service.ownerId!==request.workspaceId)fail('HOSTED_RESOURCE_CONFLICT');result.serviceId=service.id;const deploys=await api('/services/'+service.id+'/deploys?limit=1');const status=Array.isArray(deploys)?deploys[0]?.deploy?.status:null;result.state=status==='live'?'deployed':['build_failed','update_failed','canceled','pre_deploy_failed'].includes(status)?'failed':'deploying';const url=publicURL(service.serviceDetails?.url,'onrender.com');if(url)result.url=url;}
 if(result.expiresAt&&Date.parse(result.expiresAt)<=Date.now()){result.state='failed';result.databaseState='unavailable';}
 return result;
}

/** Private native recovery bridge. Not part of executeHosted's public wire.
 * It neither suspends a service nor changes an IP rule. The selected source
 * must already be observed suspended and an operator must confirm all other
 * writers stopped. Connection secrets exist only within the executor callback.
 */
// Private native-loaded original configuration. No fallback creation, renderer
// configuration input or secret projection is provided by this helper.
export function validateRecoveryConfiguration(value){
 try{
  if(!object(value)||Object.keys(value).sort().join(',')!=='envVars,origin'||publicURL(value.origin,'onrender.com')!==value.origin||!Array.isArray(value.envVars))throw Error();
  const required=['CLIENT_ORIGIN','TENANT_ID','ADMIN_SUBJECT','ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'],optional=['STATIC_PUBLICATION_ID','STATIC_REVISION_SHA256','STATIC_INDEX_SHA256'];
  const allowed=new Set([...required,...optional]),env=new Map();
  for(const entry of value.envVars){if(!object(entry)||Object.keys(entry).sort().join(',')!=='key,value'||!allowed.has(entry.key)||env.has(entry.key)||typeof entry.value!=='string'||!entry.value.length||Buffer.byteLength(entry.value)>8192||entry.value.includes('\0'))throw Error();env.set(entry.key,entry.value);}
  if(!required.every(k=>env.has(k))||![0,3].includes(optional.filter(k=>env.has(k)).length))throw Error();
  const uuid=/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;
  if(publicURL(env.get('CLIENT_ORIGIN'),'pages.dev')!==env.get('CLIENT_ORIGIN')||!uuid.test(env.get('TENANT_ID'))||!/^([a-zA-Z0-9][a-zA-Z0-9_.@-]{2,127})$/.test(env.get('ADMIN_SUBJECT'))||Buffer.byteLength(env.get('ADMIN_PASSWORD'))<12||Buffer.byteLength(env.get('ADMIN_PASSWORD'))>1024)throw Error();
  if(optional.every(k=>env.has(k))&&(!/^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i.test(env.get(optional[0]))||!hex(env.get(optional[1]))||!hex(env.get(optional[2]))))throw Error();
  const bytes=Buffer.from(env.get('FREEZE_SIGNING_KEY_BASE64'),'base64');
  try{if(bytes.toString('base64')!==env.get('FREEZE_SIGNING_KEY_BASE64'))throw Error();const key=createPrivateKey({key:bytes,format:'der',type:'pkcs8'});if(key.asymmetricKeyType!=='ed25519')throw Error();const publicKey=createPublicKey(key).export({format:'der',type:'spki'});return {origin:value.origin,keyId:sha(publicKey),publicKey:publicKey.toString('base64')};}finally{bytes.fill(0);}
 }catch{fail('HOSTED_RECOVERY_CONFIGURATION_REQUIRED');}
}
function matchRecoveryConfiguration(saved,service,expected){
 try{
  if(!Array.isArray(saved)||saved.length>100)throw Error();
  const selected=new Set(['CLIENT_ORIGIN','TENANT_ID','ADMIN_SUBJECT','ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64','STATIC_PUBLICATION_ID','STATIC_REVISION_SHA256','STATIC_INDEX_SHA256','AUTH_ORIGIN','RENDER_EXTERNAL_URL']),env=new Map();
  for(const row of saved){const e=row?.envVar;if(!e||!selected.has(e.key))continue;if(env.has(e.key)||typeof e.value!=='string')throw Error();env.set(e.key,e.value);}
  const origin=env.get('AUTH_ORIGIN')??env.get('RENDER_EXTERNAL_URL')??publicURL(service.serviceDetails?.url,'onrender.com');
  const actual={origin,envVars:[...env].filter(([key])=>!['AUTH_ORIGIN','RENDER_EXTERNAL_URL'].includes(key)).map(([key,value])=>({key,value}))};
  const authority=validateRecoveryConfiguration(actual),wanted=validateRecoveryConfiguration(expected);
  if(authority.origin!==wanted.origin||authority.keyId!==wanted.keyId||authority.publicKey!==wanted.publicKey||actual.envVars.length!==expected.envVars.length||!expected.envVars.every(e=>env.get(e.key)===e.value))throw Error();
 }catch{fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');}
}
export async function withRenderRecoveryConnection(request,options,consume){
 if(!object(request)||Object.keys(request).sort().join(',')!=='postgresId,serviceId,workspaceId'||![request.workspaceId,request.serviceId,request.postgresId].every(id)||typeof consume!=='function')fail('HOSTED_RECOVERY_REQUEST');
 validateRecoveryConfiguration(options?.expectedConfiguration);
 const cidr=options?.sourceCidr;
 const {isIP}=await import('node:net');
 if(typeof cidr!=='string'||cidr.length>64)fail('HOSTED_RECOVERY_NETWORK_REQUIRED');
 const parts=cidr.split('/'),family=isIP(parts[0]);
 if(parts.length!==2||!(family===4&&parts[1]==='32'||family===6&&parts[1]==='128'))fail('HOSTED_RECOVERY_NETWORK_REQUIRED');
 const api=transport('render',options),db=assertFree(await api('/postgres/'+request.postgresId),'postgres'),service=assertFree(await api('/services/'+request.serviceId),'services');
 if(db.id!==request.postgresId||db.owner?.id!==request.workspaceId||db.version!=='18'||db.status!=='available'||service.id!==request.serviceId||service.ownerId!==request.workspaceId||service.type!=='web_service'||service.repo!=='https://github.com/ExhibitOS/platform'||service.serviceDetails?.runtime!=='node'||service.serviceDetails?.healthCheckPath!=='/api/v1/readiness')fail('HOSTED_RESOURCE_CONFLICT');
 const expiresAt=projectDatabase(db).expiresAt;
 if(expiresAt&&Date.parse(expiresAt)<=Date.now())fail('HOSTED_RECOVERY_DATABASE_EXPIRED');
 if(!Array.isArray(db.ipAllowList)||!db.ipAllowList.some(rule=>object(rule)&&rule.cidrBlock===cidr))fail('HOSTED_RECOVERY_NETWORK_REQUIRED');
 if(service.suspended!=='suspended')fail('HOSTED_RECOVERY_WRITERS_ACTIVE');
 const info=await api('/postgres/'+request.postgresId+'/connection-info');
 if(typeof info?.internalConnectionString!=='string'||typeof info.externalConnectionString!=='string')fail('HOSTED_RESPONSE_INVALID');
 const saved=await api('/services/'+request.serviceId+'/env-vars?limit=100');
 if(!Array.isArray(saved)||saved.filter(row=>row?.envVar?.key==='DATABASE_URL').length!==1||!saved.some(row=>row?.envVar?.key==='DATABASE_URL'&&row.envVar.value===info.internalConnectionString))fail('HOSTED_RESOURCE_CONFLICT');
 matchRecoveryConfiguration(saved,service,options.expectedConfiguration);
 let external,internal;try{external=new URL(info.externalConnectionString);internal=new URL(info.internalConnectionString);}catch{fail('HOSTED_RESPONSE_INVALID');}
 if(!['postgres:','postgresql:'].includes(external.protocol)||!['postgres:','postgresql:'].includes(internal.protocol)||!external.username||!external.password||external.username!==internal.username||external.password!==internal.password||external.pathname!==internal.pathname||external.hostname.split('.')[0]!==internal.hostname||external.pathname.length<2||external.hash||external.port&&external.port!=='5432'||!/^[a-z0-9-]+\.[a-z0-9-]+-postgres\.render\.com$/.test(external.hostname))fail('HOSTED_RECOVERY_CONNECTION_INVALID');
 if([...external.searchParams].some(([key,value])=>key!=='sslmode'||!['require','verify-full'].includes(value))||external.searchParams.getAll('sslmode').length>1)fail('HOSTED_RECOVERY_TLS_REQUIRED');
 external.search='';external.searchParams.set('sslmode','verify-full');
 if(typeof options.confirmExternalWritersStopped!=='function'||await options.confirmExternalWritersStopped({workspaceId:request.workspaceId,serviceId:request.serviceId,postgresId:request.postgresId})!==true)fail('HOSTED_RECOVERY_OPERATOR_REQUIRED');
 let output;
 try{output=await consume({url:external.href});}catch{fail('HOSTED_RECOVERY_EXECUTION_FAILED');}
 // A resumed/changed remote source leaves any created local archive intact but
 // does not receive an all-writers-paused completion claim.
 const after=assertFree(await api('/services/'+request.serviceId),'services');
 if(after.id!==service.id||after.ownerId!==service.ownerId||after.suspended!=='suspended')fail('HOSTED_RECOVERY_SOURCE_CHANGED');
 const finalEnv=await api('/services/'+request.serviceId+'/env-vars?limit=100');
 if(!Array.isArray(finalEnv)||finalEnv.filter(row=>row?.envVar?.key==='DATABASE_URL').length!==1||!finalEnv.some(row=>row?.envVar?.key==='DATABASE_URL'&&row.envVar.value===info.internalConnectionString))fail('HOSTED_RESOURCE_CONFLICT');
 matchRecoveryConfiguration(finalEnv,after,options.expectedConfiguration);
 if(!object(output)||Object.keys(output).sort().join(',')!=='authenticatedManifestSha256,backupId,files,operation'||output.operation!=='created-and-verified'||typeof output.backupId!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i.test(output.backupId)||!Number.isSafeInteger(output.files)||output.files<1||output.files>100000||!hex(output.authenticatedManifestSha256))fail('HOSTED_RECOVERY_RESULT_INVALID');
 return {provider:'render',action:'local-encrypted-backup',workspaceId:request.workspaceId,serviceId:request.serviceId,postgresId:request.postgresId,...(expiresAt?{expiresAt}:{}),...output};
}
