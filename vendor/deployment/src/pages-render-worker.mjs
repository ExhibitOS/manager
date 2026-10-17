// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
// A fixed Pages module proxy; no executable code is read from the selected artifact.
import {createHash} from 'node:crypto';
const sha=value=>createHash('sha256').update(value).digest('hex');
export const DYNAMIC_META='<meta name="exhibitos-dynamic" content="same-origin">';
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
export function pagesDeploymentIdentity(manifestSha256,request){const proxy=request.staticOnly===false?createPagesProxy(request.apiOrigin):null;return {proxy,hash:proxy?sha(manifestSha256+'\n'+proxy.sha256).slice(0,40):manifestSha256.slice(0,40)};}
