// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
// A fixed Pages module proxy; no executable code is read from the selected artifact.
import {createHash} from 'node:crypto';
const sha=value=>createHash('sha256').update(value).digest('hex');
export const DYNAMIC_META='<meta name="exhibitos-dynamic" content="same-origin">';
export const STATIC_DATA_CACHE_PROFILE='static-data-sha256-24h-v1';
// Only exact, already verified publication/data descriptors are accepted. Never
// emit a wildcard: unknown paths may resolve to mutable SPA fallback HTML.
export function pagesStaticCachePolicy(descriptors){
 if(!Array.isArray(descriptors))throw Error('HOSTED_STATIC_BUNDLE_INVALID');
 const paths=[...new Set(descriptors.filter(d=>d&&typeof d.path==='string'&&typeof d.sha256==='string'&&/^data\/[a-f0-9]{64}\.(?:bin|json)$/.test(d.path)&&d.path.slice(5,69)===d.sha256).map(d=>d.path))].sort().slice(0,100);
 return {paths,headers:paths.map(path=>`/${path}\n  Cache-Control: public, max-age=86400, immutable\n`).join('\n')};
}
export function createPagesProxy(apiOrigin){
 let backend;try{backend=new URL(apiOrigin);}catch{throw Error('HOSTED_API_ORIGIN_INVALID');}
 if(backend.protocol!=='https:'||backend.username||backend.password||backend.port||backend.search||backend.hash||backend.pathname!=='/'||!(/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.onrender\.com$/.test(backend.hostname)))throw Error('HOSTED_API_ORIGIN_INVALID');
 const source=`// ExhibitOS fixed same-origin proxy v1. Backend is public configuration, never a credential.
const BACKEND = ${JSON.stringify(backend.origin)};
const unavailable = () => new Response('{"code":"DYNAMIC_UNAVAILABLE"}', {status:503,headers:{'Content-Type':'application/json','Cache-Control':'no-store'}});
export default {
 async fetch(request,env) {
  const incoming=new URL(request.url);
  if(!incoming.pathname.startsWith('/api/')) return env.ASSETS.fetch(request);
  const target=new URL(BACKEND);target.pathname=incoming.pathname;target.search=incoming.search;
  try {
   const forwarded=new Request(target,request);
   forwarded.headers.delete('Host');
   forwarded.headers.delete('Forwarded');
   forwarded.headers.delete('X-Forwarded-Host');
   forwarded.headers.delete('X-Forwarded-Proto');
   const response=await fetch(forwarded,{redirect:'manual'});
   // Preserve the runtime's native WebSocket upgrade response without reconstructing it.
   if(response.status===101) return response;
   if(response.status>=300&&response.status<400) { await response.body?.cancel(); return unavailable(); }
   const result=new Response(response.body,response);
   result.headers.set('Cache-Control','no-store');
   return result;
  } catch { return unavailable(); }
 }
};\n`;
 const routes=JSON.stringify({version:1,include:['/api/*'],exclude:[]});
 return {source,routes,apiOrigin:backend.origin,sha256:sha(source+'\n'+routes+'\n'+DYNAMIC_META)};
}
export async function appendPagesProxy(form,proxy){
 const inner=new FormData();inner.set('metadata',JSON.stringify({main_module:'worker.mjs',compatibility_date:'2025-04-01'}));inner.set('worker.mjs',new File([proxy.source],'worker.mjs',{type:'application/javascript+module'}));
 // Match Wrangler's nested multipart _worker.bundle serialization.
 const bundle=await new Response(inner).blob();form.set('_worker.bundle',new File([bundle],'_worker.bundle'));form.set('_routes.json',new File([proxy.routes],'_routes.json',{type:'application/json'}));
}
export function dynamicPublicationHTML(bytes){const source=bytes.toString('utf8');if(!/<head(?:\s[^>]*)?>/i.test(source)||source.includes('name="exhibitos-dynamic"'))throw Error('HOSTED_STATIC_BUNDLE_INVALID');return Buffer.from(source.replace(/<head(?:\s[^>]*)?>/i,match=>match+DYNAMIC_META));}
export function pagesDeploymentIdentity(manifestSha256,request){
 const proxy=request.staticOnly===false?createPagesProxy(request.apiOrigin):null,profile=request.staticCacheProfile;
 if(profile!==undefined&&request.staticOnly!==true)throw Error('HOSTED_REQUEST_INVALID');
 if(profile!==undefined&&(profile!==STATIC_DATA_CACHE_PROFILE||typeof request.staticCachePolicySha256!=='string'||!/^[a-f0-9]{64}$/.test(request.staticCachePolicySha256)))throw Error('HOSTED_REQUEST_INVALID');
 if(profile===undefined&&request.staticCachePolicySha256!==undefined)throw Error('HOSTED_REQUEST_INVALID');
 const base=proxy?manifestSha256+'\n'+proxy.sha256:manifestSha256;
 return {proxy,hash:profile?sha(base+'\n'+profile+'\n'+request.staticCachePolicySha256).slice(0,40):proxy?sha(base).slice(0,40):manifestSha256.slice(0,40)};
}
