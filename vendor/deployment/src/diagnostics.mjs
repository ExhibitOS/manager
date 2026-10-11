// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {execFile} from 'node:child_process';
const MAX_BYTES=262144, MAX_CONTAINERS=64, TOTAL_MS=20000;
const idRE=/^[a-f0-9]{64}$/, nameRE=/^[a-z0-9][a-z0-9_-]{0,47}$/;
const errorCodes=new Set(['USAGE','TARGET_INVALID','ENGINE_OUTPUT_INVALID','ENGINE_UNAVAILABLE','ENGINE_TIMEOUT','ENGINE_OBSERVATION_FAILED','APPLICATION_BINDING_UNVERIFIED','APPLICATION_RESPONSE_INVALID','APPLICATION_TIMEOUT','APPLICATION_UNAVAILABLE','ENGINE_LOCALITY_UNVERIFIED','ENGINE_SELECTION_CHANGED']);
const states=new Set(['created','restarting','running','removing','paused','exited','dead']);
const keys=['id','project','service','imageConfigDigest','running','state','health','exitCode','restarts'].sort().join(',');
const template='{ "id":{{json .Id}}, "project":{{json (index .Config.Labels "com.docker.compose.project")}}, "service":{{json (index .Config.Labels "com.docker.compose.service")}}, "imageConfigDigest":{{json .Image}}, "running":{{json .State.Running}}, "state":{{json .State.Status}}, "health":{{if .State.Health}}{{json .State.Health.Status}}{{else}}null{{end}}, "exitCode":{{json .State.ExitCode}}, "restarts":{{json .RestartCount}} }';
export class DiagnosticError extends Error {constructor(code){super(code);this.code=code;}}
const fail=code=>{throw new DiagnosticError(code);};
export function validateTarget(target){
 if(!target||Object.keys(target).sort().join(',')!=='project,services'||typeof target.project!=='string'||!nameRE.test(target.project)||!Array.isArray(target.services)||target.services.length<1||target.services.length>16||target.services.some(s=>typeof s!=='string'||!nameRE.test(s))||new Set(target.services).size!==target.services.length)fail('TARGET_INVALID');
 return Object.freeze({project:target.project,services:Object.freeze([...target.services])});
}
export function parseIds(text){
 if(typeof text!=='string'||Buffer.byteLength(text)>MAX_BYTES)fail('ENGINE_OUTPUT_INVALID');
 const ids=text.trim()?text.trim().split(/\r?\n/):[];
 if(ids.length>MAX_CONTAINERS||ids.some(id=>!idRE.test(id))||new Set(ids).size!==ids.length)fail('ENGINE_OUTPUT_INVALID');
 return ids.sort();
}
export function parseInspection(text,id,project){
 if(typeof text!=='string'||Buffer.byteLength(text)>MAX_BYTES)fail('ENGINE_OUTPUT_INVALID');
 let r;try{r=JSON.parse(text);}catch{fail('ENGINE_OUTPUT_INVALID');}
 const tokens=text.match(/"(?:[^"\\]|\\.)*"\s*:/g)??[];
 if(tokens.length!==9)fail('ENGINE_OUTPUT_INVALID');
 for(const key of keys.split(','))if(tokens.filter(t=>t.replace(/\s*:/,'')===JSON.stringify(key)).length!==1)fail('ENGINE_OUTPUT_INVALID');
 if(!r||Array.isArray(r)||Object.keys(r).sort().join(',')!==keys||r.id!==id||r.project!==project||typeof r.service!=='string'||!nameRE.test(r.service)||typeof r.imageConfigDigest!=='string'||!/^sha256:[a-f0-9]{64}$/.test(r.imageConfigDigest)||typeof r.running!=='boolean'||!states.has(r.state)||!(r.health===null||['healthy','unhealthy','starting'].includes(r.health))||!Number.isInteger(r.exitCode)||r.exitCode<0||r.exitCode>2147483647||!Number.isSafeInteger(r.restarts)||r.restarts<0)fail('ENGINE_OUTPUT_INVALID');
 return r;
}
export function dockerRead(args,{timeout,env}){
 return new Promise((resolve,reject)=>execFile('docker',args,{shell:false,windowsHide:true,timeout,env,maxBuffer:MAX_BYTES,encoding:'utf8'},(err,stdout)=>{
  if(err)return reject(new DiagnosticError(err.code==='ENOENT'?'ENGINE_UNAVAILABLE':err.killed?'ENGINE_TIMEOUT':'ENGINE_OBSERVATION_FAILED'));
  resolve(stdout);
 }));
}
export async function diagnose(input,{run=dockerRead,clock=()=>performance.now(),observedClock=Date.now}={}){
 const target=validateTarget(input),start=clock(),observedStartMs=observedClock(),deadline=start+TOTAL_MS;
 const read=async args=>{const remaining=deadline-clock();if(remaining<=0)fail('ENGINE_TIMEOUT');try{const result=await run(args,{timeout:Math.min(8000,remaining)});if(clock()>deadline)fail('ENGINE_TIMEOUT');return result;}catch(e){fail(e instanceof DiagnosticError?e.code:'ENGINE_OBSERVATION_FAILED');}};
 const census=()=>read(['ps','--all','--no-trunc','--filter',`label=com.docker.compose.project=${target.project}`,'--format','{{.ID}}']);
 const before=parseIds(await census()),records=[];
 for(const id of before)records.push(parseInspection(await read(['inspect','--format',template,id]),id,target.project));
 const after=parseIds(await census()),changed=before.join(',')!==after.join(',');
 let unexpected=0;
 const services=target.services.map(name=>{
  const rs=records.filter(r=>r.service===name);
  if(rs.length!==1)return {service:name,state:rs.length?'duplicate':'missing',containerHealth:'unknown',containers:rs.length};
  const r=rs[0];const health=r.running&&r.state==='running'?(r.health==='healthy'?'healthy':r.health==='unhealthy'?'unhealthy':'unknown'):'unknown';
  return {service:name,state:r.state,containerHealth:health,observedEngineHealth:r.health??'not-configured',containers:1,id:r.id,imageConfigDigest:r.imageConfigDigest,exitCode:r.exitCode,restarts:r.restarts};
 });
 for(const r of records)if(!target.services.includes(r.service))unexpected++;
 const health=changed||unexpected||services.some(s=>s.containerHealth==='unknown')?'unknown':services.some(s=>s.containerHealth==='unhealthy')?'unhealthy':'healthy';
 if(clock()>deadline)fail('ENGINE_TIMEOUT');
 return {version:1,scope:'readonly-compose-container-observations',containerHealth:health,applicationReadiness:'not-verified',atomicSnapshot:false,censusChanged:changed,unexpectedContainers:unexpected,observedStartMs,observedEndMs:observedClock(),services};
}
export function safeError(error){return {version:1,state:'observation-failed',code:error instanceof DiagnosticError&&errorCodes.has(error.code)?error.code:'ENGINE_OBSERVATION_FAILED',applicationReadiness:'not-verified'};}
