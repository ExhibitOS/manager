// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {execFile} from 'node:child_process';
import {boundedJSON} from './bounded-json.mjs';
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
export async function diagnose(input,{run=dockerRead,clock=()=>performance.now(),observedClock=Date.now,resources=false}={}){
 if(typeof resources!=='boolean')fail('TARGET_INVALID');
 const target=validateTarget(input),start=clock(),observedStartMs=observedClock(),deadline=start+TOTAL_MS;
 const read=async args=>{const remaining=deadline-clock();if(remaining<=0)fail('ENGINE_TIMEOUT');try{const result=await run(args,{timeout:Math.min(8000,remaining)});if(clock()>deadline)fail('ENGINE_TIMEOUT');return result;}catch(e){fail(e instanceof DiagnosticError?e.code:'ENGINE_OBSERVATION_FAILED');}};
 const census=()=>read(['ps','--all','--no-trunc','--filter',`label=com.docker.compose.project=${target.project}`,'--format','{{.ID}}']);
 const before=parseIds(await census()),records=[];
 for(const id of before)records.push(parseInspection(await read(['inspect','--format',template,id]),id,target.project));
 let resourceObservation=resources?await observeResources(records,target,{run,clock,observedClock,deadline}):null;
 const after=parseIds(await census()),changed=before.join(',')!==after.join(',');
 if(resourceObservation&&changed)resourceObservation={version:1,state:'unknown',scope:resourceObservation.scope,reason:'census-changed',atomicSnapshot:false,thresholds:resourceThresholds,services:[]};
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
 return {version:1,scope:'readonly-compose-container-observations',containerHealth:health,applicationReadiness:'not-verified',atomicSnapshot:false,censusChanged:changed,unexpectedContainers:unexpected,observedStartMs,observedEndMs:observedClock(),services,...(resources?{resourceObservation}:{})};
}
export function safeError(error){return {version:1,state:'observation-failed',code:error instanceof DiagnosticError&&errorCodes.has(error.code)?error.code:'ENGINE_OBSERVATION_FAILED',applicationReadiness:'not-verified'};}

const resourceThresholds=Object.freeze({cpuPercentOneCore:90,memoryPercent:85,writableLayerBytes:1073741824});
const statsTemplate='{ "id":{{json .ID}}, "cpu":{{json .CPUPerc}}, "memory":{{json .MemUsage}}, "memoryPercent":{{json .MemPerc}} }';
const sizeTemplate='{ "id":{{json .Id}}, "project":{{json (index .Config.Labels "com.docker.compose.project")}}, "writableBytes":{{json .SizeRw}}, "rootFilesystemBytes":{{json .SizeRootFs}} }';
const exact=(v,names)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===[...names].sort().join(',');
function percent(v,max){if(typeof v!=='string'||!/^\d{1,6}(?:\.\d{1,3})?%$/.test(v))fail('ENGINE_OUTPUT_INVALID');const n=Number(v.slice(0,-1));if(!Number.isFinite(n)||n<0||n>max)fail('ENGINE_OUTPUT_INVALID');return n;}
function memoryBytes(v){
 const match=typeof v==='string'&&/^(\d{1,16}(?:\.\d{1,4})?)(B|kB|MB|GB|TB|KiB|MiB|GiB|TiB)$/.exec(v);
 if(!match)fail('ENGINE_OUTPUT_INVALID');const units={B:1,kB:1000,MB:1e6,GB:1e9,TB:1e12,KiB:1024,MiB:1048576,GiB:1073741824,TiB:1099511627776};
 const value=Math.round(Number(match[1])*units[match[2]]);if(!Number.isSafeInteger(value)||value<0)fail('ENGINE_OUTPUT_INVALID');return value;
}
export function parseResourceStats(text,ids){
 if(typeof text!=='string'||Buffer.byteLength(text)>MAX_BYTES)fail('ENGINE_OUTPUT_INVALID');const lines=text.trim()?text.trim().split(/\r?\n/):[];
 if(lines.length!==ids.length||lines.length>MAX_CONTAINERS)fail('ENGINE_OUTPUT_INVALID');const seen=new Set();
 return lines.map(line=>{let v;try{v=boundedJSON(line,MAX_BYTES);}catch{fail('ENGINE_OUTPUT_INVALID');}
  if(!exact(v,['id','cpu','memory','memoryPercent'])||!ids.includes(v.id)||seen.has(v.id)||typeof v.memory!=='string')fail('ENGINE_OUTPUT_INVALID');seen.add(v.id);
  const parts=v.memory.split(' / ');if(parts.length!==2)fail('ENGINE_OUTPUT_INVALID');const used=memoryBytes(parts[0]),limit=memoryBytes(parts[1]);
  if(limit===0||used>limit)fail('ENGINE_OUTPUT_INVALID');return {id:v.id,cpuPercentOneCore:percent(v.cpu,262144),memoryUsedBytesEstimate:used,memoryLimitBytesEstimate:limit,memoryPercent:percent(v.memoryPercent,100)};
 });
}
export function parseWritableStorage(text,id,project){
 let v;try{v=boundedJSON(text,MAX_BYTES);}catch{fail('ENGINE_OUTPUT_INVALID');}
 if(!exact(v,['id','project','writableBytes','rootFilesystemBytes'])||v.id!==id||v.project!==project||!Number.isSafeInteger(v.writableBytes)||!Number.isSafeInteger(v.rootFilesystemBytes)||v.writableBytes<0||v.rootFilesystemBytes<v.writableBytes)fail('ENGINE_OUTPUT_INVALID');
 return {writableLayerBytes:v.writableBytes,rootFilesystemBytes:v.rootFilesystemBytes};
}
async function observeResources(records,target,{run,clock,observedClock,deadline}){
 const observation={version:1,scope:'declared-project-container-point-observations',atomicSnapshot:false,observedStartMs:observedClock(),thresholds:resourceThresholds,services:[],storageCapacity:{state:'unknown',reason:'configured-data-volume-observation-required'},queue:{state:'unknown',reason:'authenticated-tenant-aggregate-required'},errors:{state:'unknown'}};
 const declared=records.length===target.services.length&&target.services.every(service=>records.filter(r=>r.service===service).length===1);
 if(!declared)return {...observation,state:'unknown',reason:'service-binding-unverified',observedEndMs:observedClock()};
 // Preserve eight seconds for the original final census; metrics have a separate five-second slice.
 const end=Math.min(deadline-8000,clock()+5000),read=async args=>{const remaining=end-clock();if(remaining<=0)throw new DiagnosticError('ENGINE_TIMEOUT');const value=await run(args,{timeout:Math.min(2000,remaining)});if(clock()>end)throw new DiagnosticError('ENGINE_TIMEOUT');return value;};
 let stats=[];const running=records.filter(r=>r.running&&r.state==='running').map(r=>r.id).sort();
 if(running.length)try{stats=parseResourceStats(await read(['stats','--no-stream','--no-trunc','--format',statsTemplate,...running]),running);}catch{/* Unknown never becomes a zero measurement. */}
 for(const r of records){
  const cpu=stats.find(s=>s.id===r.id);let disk=null;
  try{disk=parseWritableStorage(await read(['inspect','--size','--format',sizeTemplate,r.id]),r.id,target.project);}catch{/* Unsupported engine, timeout and invalid output are bounded unknowns. */}
  const metrics={cpu:cpu?{state:'observed',percentOneCore:cpu.cpuPercentOneCore,alert:cpu.cpuPercentOneCore>=resourceThresholds.cpuPercentOneCore?'warning':'normal'}:{state:'unknown'},memory:cpu?{state:'observed',usedBytesEstimate:cpu.memoryUsedBytesEstimate,limitBytesEstimate:cpu.memoryLimitBytesEstimate,percent:cpu.memoryPercent,alert:cpu.memoryPercent>=resourceThresholds.memoryPercent?'warning':'normal'}:{state:'unknown'},writableLayer:disk?{state:'observed',...disk,alert:disk.writableLayerBytes>=resourceThresholds.writableLayerBytes?'warning':'normal'}:{state:'unknown'}};
  observation.services.push({service:r.service,containerId:r.id,metrics});
 }
 const failed=records.filter(r=>r.exitCode!==0).length,restarts=records.reduce((n,r)=>n+BigInt(r.restarts),0n);
 observation.errors=restarts<=BigInt(Number.MAX_SAFE_INTEGER)?{state:'observed',scope:'current-container-history-not-error-rate',nonzeroExitContainers:failed,restarts:Number(restarts),alert:failed?'warning':'normal'}:{state:'unknown'};
 return {...observation,state:observation.services.every(s=>Object.values(s.metrics).every(m=>m.state==='observed'))?'observed':'partial',observedEndMs:observedClock()};
}
