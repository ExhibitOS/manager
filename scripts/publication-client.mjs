// SPDX-License-Identifier: Apache-2.0
const LIMIT=256*1024;
const UUID=/^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;
const TOKEN=/^[A-Za-z0-9_-]{43}$/;
class LocalCmsError extends Error {}
const fail=code=>{throw new LocalCmsError(code);};
const record=x=>x!==null&&typeof x==='object'&&!Array.isArray(x);
const closed=(x,keys)=>record(x)&&Object.keys(x).length===keys.length&&keys.every(k=>Object.hasOwn(x,k));
const iso=x=>typeof x==='string'&&x.length===24&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
async function boundedJson(response,code){
 if(!response?.ok||!response.body||response.redirected)fail(code);
 if(!/^application\/json(?:\s*;|$)/i.test(response.headers.get('content-type')??''))fail(code);
 const declared=response.headers.get('content-length');if(declared!==null&&(!/^\d+$/.test(declared)||Number(declared)>LIMIT))fail(code);
 const reader=response.body.getReader(),chunks=[];let size=0;
 try{for(;;){const{done,value}=await reader.read();if(done)break;size+=value.length;if(size>LIMIT)fail(code);chunks.push(value);}return JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(Buffer.concat(chunks)));}
 catch{await reader.cancel().catch(()=>{});fail(code);}finally{reader.releaseLock();}
}
/** Native-only short-lived session. Never return cookies, CSRF, credentials or server errors. */
export async function listLocalPublications(options={}){
 let cookie,csrf,request,result,primary;
 try{
  const{origin,tenantId,subject,password,fetcher=fetch}=options??{};
  let base;try{base=new URL(origin);}catch{fail('LOCAL_CMS_ORIGIN');}
  if(!['http:','https:'].includes(base.protocol)||!['localhost','127.0.0.1','[::1]'].includes(base.hostname)||base.username||base.password||base.pathname!=='/'||base.search||base.hash||typeof origin!=='string'||![base.origin,base.origin+'/'].includes(origin))fail('LOCAL_CMS_ORIGIN');
  if(!UUID.test(tenantId??'')||typeof subject!=='string'||subject.length<3||subject.length>128||typeof password!=='string'||password.length<12||password.length>1024||typeof fetcher!=='function')fail('LOCAL_CMS_INPUT');
  request=async(path,{method='GET',body,logout=false}={},code)=>{
   try{return await fetcher(new URL(path,base),{method,redirect:'error',credentials:'omit',cache:'no-store',signal:AbortSignal.timeout(30000),headers:{accept:'application/json',origin:base.origin,...(body?{'content-type':'application/json'}:{}),...(cookie?{cookie}:{}),...(logout?{'x-csrf-token':csrf}:{})},...(body?{body:JSON.stringify(body)}:{})});}catch{fail(code);}
  };
  const login=await request('/api/v1/auth/login',{method:'POST',body:{tenantId,subject,password}},'LOCAL_CMS_LOGIN');
  if(!login?.ok)fail('LOCAL_CMS_LOGIN');
  const name=base.protocol==='https:'?'__Host-exhibitos_session':'exhibitos_local_session';
  const cookies=login.headers.getSetCookie?.()??[login.headers.get('set-cookie')].filter(Boolean);
  if(cookies.length!==1)fail('LOCAL_CMS_SESSION');
  const match=cookies[0].match(new RegExp('^'+name+'=([A-Za-z0-9_-]{43})(?:;|$)'));
  if(!match)fail('LOCAL_CMS_SESSION');cookie=name+'='+match[1];
  const authenticated=await boundedJson(login,'LOCAL_CMS_LOGIN');if(!closed(authenticated,['authenticated'])||authenticated.authenticated!==true)fail('LOCAL_CMS_LOGIN');
  const session=await boundedJson(await request('/api/v1/auth/session',{},'LOCAL_CMS_SESSION'),'LOCAL_CMS_SESSION');
  // Retain only the validated synchronizer token for a finally-block logout, including tenant refusal.
  if(TOKEN.test(session?.csrfToken??''))csrf=session.csrfToken;
  if(!closed(session,['id','userId','tenantId','role','csrfToken','expiresAt'])||!UUID.test(session.id)||!UUID.test(session.userId)||!csrf||!iso(session.expiresAt)||Date.parse(session.expiresAt)<=Date.now())fail('LOCAL_CMS_SESSION');
  if(session.tenantId!==tenantId)fail('LOCAL_CMS_TENANT');
  if(!['admin','curator','artist'].includes(session.role))fail('LOCAL_CMS_FORBIDDEN');
  const catalog=await boundedJson(await request(`/api/v1/tenants/${tenantId}/studio/publications`,{},'LOCAL_CMS_CATALOG'),'LOCAL_CMS_CATALOG');
  if(!closed(catalog,['items'])||!Array.isArray(catalog.items)||catalog.items.length>100)fail('LOCAL_CMS_CATALOG');
  const ids=new Set(),items=[];
  for(const item of catalog.items){
   if(!closed(item,['publicationId','exhibitionId','title','revisionSha256','publishedAt','status'])||!UUID.test(item.publicationId)||!UUID.test(item.exhibitionId)||typeof item.title!=='string'||item.title.length>256||/[\u0000-\u001f\u007f]/.test(item.title)||!/^[a-f0-9]{64}$/.test(item.revisionSha256)||!iso(item.publishedAt)||item.status!=='published'||ids.has(item.publicationId))fail('LOCAL_CMS_CATALOG');
   ids.add(item.publicationId);items.push({publicationId:item.publicationId,exhibitionId:item.exhibitionId,title:item.title,revisionSha256:item.revisionSha256,publishedAt:item.publishedAt,status:'published'});
  }
  result={items};
 }catch(error){primary=error instanceof LocalCmsError?error:new LocalCmsError('LOCAL_CMS_REQUEST');}
 finally{
  // A successful cookie issuance can precede a malformed login/session response.
  // Recover only a valid CSRF token for best-effort revocation; never expose it.
  if(cookie&&!csrf&&request){try{const s=await boundedJson(await request('/api/v1/auth/session',{},'LOCAL_CMS_SESSION'),'LOCAL_CMS_SESSION');if(TOKEN.test(s?.csrfToken??''))csrf=s.csrfToken;}catch{}}
  if(cookie&&csrf&&request){try{const result=await boundedJson(await request('/api/v1/auth/logout',{method:'POST',logout:true},'LOCAL_CMS_LOGOUT'),'LOCAL_CMS_LOGOUT');if(!closed(result,['loggedOut'])||result.loggedOut!==true)fail('LOCAL_CMS_LOGOUT');}catch{primary??=new LocalCmsError('LOCAL_CMS_LOGOUT');}}
  cookie=undefined;csrf=undefined;
 }
 if(primary)throw primary;return result;
}
