// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {createHash} from 'node:crypto';
import {DiagnosticError} from './diagnostics.mjs';
import {boundedJSON} from './bounded-json.mjs';
const fail=code=>{throw new DiagnosticError(code);};
const selectionKeys=['DOCKER_CONTEXT','DOCKER_HOST','DOCKER_CONFIG','DOCKER_TLS','DOCKER_TLS_VERIFY','DOCKER_CERT_PATH','PATH'];
const selection=env=>selectionKeys.map(key=>env[key]??null);
const contextName=text=>{if(typeof text!=='string'||Buffer.byteLength(text)>256)fail('ENGINE_LOCALITY_UNVERIFIED');const name=text.trim();if(!/^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$/.test(name))fail('ENGINE_LOCALITY_UNVERIFIED');return name;};
export function localEndpoint(text){
 if(typeof text!=='string'||Buffer.byteLength(text)>1024||/[\x00-\x20\x7f?#]/.test(text)||!(text.startsWith('unix:///')&&text.length>8||/^npipe:\/\/\/\/\.\/pipe\/[a-zA-Z0-9_.-]+$/.test(text)))fail('ENGINE_LOCALITY_UNVERIFIED');
 return text;
}
/** Effective selection: DOCKER_CONTEXT, then DOCKER_HOST, then current context. No credential fields are read. */
async function resolve(run,env){
 if(env.DOCKER_CONTEXT){const context=contextName(env.DOCKER_CONTEXT);return {kind:'context',context,endpoint:await endpoint(run,context)};}
 if(env.DOCKER_HOST)return {kind:'host',endpoint:localEndpoint(env.DOCKER_HOST)};
 const context=contextName(await run(['context','show']));return {kind:'current-context',context,endpoint:await endpoint(run,context)};
}
async function endpoint(run,context){
 let host;try{host=boundedJSON(await run(['context','inspect','--format','{{json .Endpoints.docker.Host}}',context]),2048);}catch{fail('ENGINE_LOCALITY_UNVERIFIED');}
 return localEndpoint(host);
}
/** Pin all observations to one local transport and frozen environment. Local daemon/proxy trust remains an assumption. */
export async function bindLocalEngine(run,getEnvironment=()=>process.env){
 const frozen=Object.freeze({...getEnvironment()}),beforeSelection=selection(frozen);
 const selected=await resolve(args=>run(args,{env:frozen}),frozen);
 const pinnedEnvironment={...frozen};delete pinnedEnvironment.DOCKER_CONTEXT;delete pinnedEnvironment.DOCKER_HOST;
 // TLS settings cannot describe the admitted Unix/named-pipe transport and must not alter explicit host selection.
 for(const key of ['DOCKER_TLS','DOCKER_TLS_VERIFY','DOCKER_CERT_PATH'])delete pinnedEnvironment[key];
 Object.freeze(pinnedEnvironment);
 return Object.freeze({
  endpointDigest:createHash('sha256').update(selected.endpoint).digest('hex'),
  managerEnvironment:()=>Object.freeze({...pinnedEnvironment,DOCKER_HOST:selected.endpoint}),
  run:(args,options)=>run(['--host',selected.endpoint,...args],{...options,env:pinnedEnvironment}),
  verify:async()=>{
   if(JSON.stringify(selection(getEnvironment()))!==JSON.stringify(beforeSelection))fail('ENGINE_SELECTION_CHANGED');
   const after=await resolve(args=>run(args,{env:frozen}),frozen);
   if(JSON.stringify(after)!==JSON.stringify(selected))fail('ENGINE_SELECTION_CHANGED');
  }
 });
}
