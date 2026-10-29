// SPDX-License-Identifier: Apache-2.0
// Fixed native static-only entry. No code is loaded from the selected exhibition.
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import {executeHosted,inspectStaticBundle} from '../vendor/deployment/src/pages-render.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
import {STATIC_DATA_CACHE_PROFILE} from '../vendor/deployment/src/pages-render-worker.mjs';

const fail=code=>{throw Error(code);};
const publicCodes=new Set(['HOSTED_PUBLIC_CONTENT_MISMATCH','HOSTED_PUBLIC_CONTENT_UNAVAILABLE','HOSTED_CACHE_HEADERS_MISMATCH','HOSTED_CACHE_POLICY_NOT_READY','HOSTED_REQUEST_INVALID','HOSTED_TOKEN_INVALID','HOSTED_RESPONSE_INVALID','HOSTED_OPERATION_UNCERTAIN']);
// Provider error frames are fixed product codes; raw exceptions never cross IPC.
const providerCodes=new Set(['HOSTED_API_ORIGIN_INVALID','HOSTED_BUSY','HOSTED_CANONICAL_ORIGIN_CONFLICT','HOSTED_CONNECTION_FAILED','HOSTED_DEPLOYMENT_BRANCH_CONFLICT','HOSTED_OPERATION_UNCERTAIN','HOSTED_PAGINATION_LIMIT','HOSTED_PAID_PLAN_REFUSED','HOSTED_PERMISSION_REQUIRED','HOSTED_PROJECT_CONFLICT','HOSTED_RATE_LIMITED','HOSTED_REDIRECT_REFUSED','HOSTED_REMOTE_REJECTED','HOSTED_REQUEST_INVALID','HOSTED_RESOURCE_CONFLICT','HOSTED_RESPONSE_INVALID','HOSTED_RESULT_UNCERTAIN','HOSTED_SOURCE_INVALID','HOSTED_SOURCE_REQUIRED','HOSTED_STATIC_BUNDLE_CHANGED','HOSTED_STATIC_BUNDLE_INVALID','HOSTED_TOKEN_INVALID','HOSTED_TOKEN_REQUIRED']);
export function nativeHostingFailure(error){
 const code=error?.code??error?.message;
 return publicCodes.has(code)||providerCodes.has(code)?code:'HOSTED_OPERATION_UNCERTAIN';
}
export async function verifyPublicStatic(bundle,result,fetch=globalThis.fetch,cacheProfile,cacheMismatchCode='HOSTED_PUBLIC_CONTENT_MISMATCH'){
 if(result.provider!=='cloudflare'||result.state!=='ready'||typeof result.projectName!=='string'||!/^[a-z0-9](?:[a-z0-9-]{0,57}[a-z0-9])?$/.test(result.projectName)||result.url!==`https://${result.projectName}.pages.dev`)fail('HOSTED_RESPONSE_INVALID');
 let bytes=0,headersMismatch=false;
 for(const file of bundle.files){
  // inspectStaticBundle already admitted exact safe relative names and bounds.
  if(typeof file.path!=='string'||!file.path.split('/').every(p=>/^[A-Za-z0-9_.-]+$/.test(p)&&p!=='.'&&p!=='..')||!Number.isSafeInteger(file.bytes)||file.bytes<0||file.bytes>25*1024**2||!/^[a-f0-9]{64}$/.test(file.sha256))fail('HOSTED_STATIC_BUNDLE_INVALID');
  let response;
  try{response=await fetch(result.url+'/'+file.path,{redirect:'manual',credentials:'omit',referrerPolicy:'no-referrer',cache:'no-store',headers:{'Cache-Control':'no-cache'},signal:AbortSignal.timeout(30000)});}
  catch{fail('HOSTED_PUBLIC_CONTENT_UNAVAILABLE');}
  if(response.status!==200||!response.body){await response.body?.cancel();fail('HOSTED_PUBLIC_CONTENT_UNAVAILABLE');}
  if(cacheProfile===STATIC_DATA_CACHE_PROFILE&&bundle.cachePolicy.paths.includes(file.path)){
   const directives=(response.headers.get('cache-control')??'').toLowerCase().split(',').map(v=>v.trim());
   if(directives.length!==3||new Set(directives).size!==3||!directives.includes('public')||!directives.includes('max-age=86400')||!directives.includes('immutable'))headersMismatch=true;
  }
  const reader=response.body.getReader(),digest=createHash('sha256');let count=0;
  try{
   for(;;){const {done,value}=await reader.read();if(done)break;count+=value.byteLength;if(count>file.bytes)fail('HOSTED_PUBLIC_CONTENT_MISMATCH');digest.update(value);}
  }catch(error){await reader.cancel().catch(()=>{});if(error?.message==='HOSTED_PUBLIC_CONTENT_MISMATCH')throw error;fail('HOSTED_PUBLIC_CONTENT_UNAVAILABLE');}
  finally{reader.releaseLock();}
  if(count!==file.bytes||digest.digest('hex')!==file.sha256)fail('HOSTED_PUBLIC_CONTENT_MISMATCH');
  bytes+=count;
 }
 if(headersMismatch)fail(cacheMismatchCode);
 return {manifestSha256:bundle.manifestSha256,files:bundle.files.length,bytes};
}
export async function runNativeHosting(request,tokens,options={}){
 if(!request||typeof request!=='object'||Array.isArray(request)||Object.keys(request).some(k=>!['provider','action','accountId','projectName','sourcePath','expectedManifestSha256','staticOnly','staticCacheProfile','staticCachePolicySha256'].includes(k))||request.provider!=='cloudflare'||request.staticOnly!==true||!['accounts','deploy-static','reconcile'].includes(request.action))fail('HOSTED_REQUEST_INVALID');
 if(request.staticCacheProfile!==undefined&&(request.staticCacheProfile!==STATIC_DATA_CACHE_PROFILE||typeof request.staticCachePolicySha256!=='string'||!/^[a-f0-9]{64}$/.test(request.staticCachePolicySha256)))fail('HOSTED_REQUEST_INVALID');
 if(request.staticCacheProfile===undefined&&request.staticCachePolicySha256!==undefined)fail('HOSTED_REQUEST_INVALID');
 if(!tokens||Object.keys(tokens).join(',')!=='cloudflare'||typeof tokens.cloudflare!=='string'||!/^[!-~]{1,4096}$/.test(tokens.cloudflare))fail('HOSTED_TOKEN_INVALID');
 if(request.action==='accounts'&&Object.keys(request).some(k=>!['provider','action','staticOnly'].includes(k)))fail('HOSTED_REQUEST_INVALID');
 const bundle=request.action==='accounts'?null:await inspectStaticBundle(request.sourcePath,{expectedManifestSha256:request.expectedManifestSha256});
 if(bundle&&request.staticCacheProfile!==undefined&&createHash('sha256').update(bundle.cachePolicy.headers).digest('hex')!==request.staticCachePolicySha256)fail('HOSTED_STATIC_BUNDLE_CHANGED');
 // Draft-only opt-in: the native owner keeps marked work blocked until the
 // exporter, rollback and actual provider behavior are independently qualified.
 const marked=options.markerDraft===true&&!!bundle?.cacheMarker;
 if(bundle?.cacheMarker&&!marked)fail('HOSTED_CACHE_POLICY_NOT_READY');
 if(marked&&request.staticCacheProfile!==undefined)fail('HOSTED_STATIC_BUNDLE_INVALID');
 const effective=marked?{...request,staticCacheMarkerEnabled:true}:request;
 const execute=options.execute??executeHosted,fetch=options.fetch??globalThis.fetch;
 let result=await execute(effective,{fetch,tokens});
 if(!result||result.provider!=='cloudflare'||!['ready','deploying','uncertain','failed'].includes(result.state))fail('HOSTED_RESPONSE_INVALID');
 if(bundle&&result.state==='ready'){
  if(result.projectName!==request.projectName)fail('HOSTED_RESPONSE_INVALID');
  const current=await inspectStaticBundle(request.sourcePath,{expectedManifestSha256:bundle.manifestSha256});
  try{result.verification=await verifyPublicStatic(current,result,fetch,marked?STATIC_DATA_CACHE_PROFILE:request.staticCacheProfile,marked?'HOSTED_CACHE_HEADERS_MISMATCH':undefined);}
  catch(error){
   // Only an explicit deploy may repair, once, after the exact public bytes
   // were observed with incorrect headers on the reused current deployment.
   if(!marked||request.action!=='deploy-static'||result.reusedCurrent!==true||error?.message!=='HOSTED_CACHE_HEADERS_MISMATCH')throw error;
   result=await execute(effective,{fetch,tokens,markerRepair:{deploymentId:result.deploymentId}});
   if(!result||result.provider!=='cloudflare'||!['ready','deploying','uncertain','failed'].includes(result.state))fail('HOSTED_RESPONSE_INVALID');
   if(result.state==='ready'){
    if(result.projectName!==request.projectName)fail('HOSTED_RESPONSE_INVALID');
    const after=await inspectStaticBundle(request.sourcePath,{expectedManifestSha256:bundle.manifestSha256});
    result.verification=await verifyPublicStatic(after,result,fetch,STATIC_DATA_CACHE_PROFILE,'HOSTED_CACHE_HEADERS_MISMATCH');
   }
  }
 }
 return result;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const chunks=[];let bytes=0;
  for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>131072)fail('HOSTED_REQUEST_INVALID');chunks.push(chunk);}
  const input=boundedJSON(Buffer.concat(chunks).toString('utf8'),131072);
  if(!input||Object.keys(input).sort().join(',')!=='request,tokens')fail('HOSTED_REQUEST_INVALID');
  const result=await runNativeHosting(input.request,input.tokens),output=JSON.stringify(result);
  if(Buffer.byteLength(output)>262144||output.includes(input.tokens.cloudflare))fail('HOSTED_RESPONSE_INVALID');
  process.stdout.write(output);
 }catch(error){process.stdout.write(JSON.stringify({errorCode:nativeHostingFailure(error)}));process.exitCode=1;}
}
