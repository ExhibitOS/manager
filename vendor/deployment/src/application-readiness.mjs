// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {request} from 'node:http';
import {diagnose,dockerRead,DiagnosticError,parseIds,validateTarget} from './diagnostics.mjs';
import {boundedJSON} from './bounded-json.mjs';
import {bindLocalEngine} from './engine-locality.mjs';
const MAX_BODY=16384,TOTAL_MS=20000,serviceNames=['platform','api','database','web','storage'];
const portTemplate='{ "id":{{json .Id}}, "project":{{json (index .Config.Labels "com.docker.compose.project")}}, "service":{{json (index .Config.Labels "com.docker.compose.service")}}, "imageConfigDigest":{{json .Image}}, "running":{{json .State.Running}}, "state":{{json .State.Status}}, "restarts":{{json .RestartCount}}, "bindings":{{json (index .NetworkSettings.Ports "8080/tcp")}} }';
const fail=code=>{throw new DiagnosticError(code);};
const exact=(o,k)=>o&&typeof o==='object'&&!Array.isArray(o)&&Object.keys(o).sort().join(',')===k.split(',').sort().join(',');
export function parsePortInspection(text,expected){
 let r;try{r=boundedJSON(text);}catch{fail('ENGINE_OUTPUT_INVALID');}
 if(!exact(r,'id,project,service,imageConfigDigest,running,state,restarts,bindings')||r.id!==expected.id||r.project!==expected.project||r.service!==expected.service||r.imageConfigDigest!==expected.imageConfigDigest||r.running!==true||r.state!=='running'||r.restarts!==expected.restarts||!(r.bindings===null||Array.isArray(r.bindings)&&r.bindings.length<=4))fail('APPLICATION_BINDING_UNVERIFIED');
 if(r.bindings===null||r.bindings.length!==1)return null;
 const b=r.bindings[0];if(!exact(b,'HostIp,HostPort')||b.HostIp!=='127.0.0.1'||typeof b.HostPort!=='string'||!(/^[1-9][0-9]{0,4}$/).test(b.HostPort)||Number(b.HostPort)>65535)return null;
 return {id:r.id,project:r.project,service:r.service,imageConfigDigest:r.imageConfigDigest,running:true,state:'running',restarts:r.restarts,host:'127.0.0.1',port:Number(b.HostPort),containerPort:8080};
}
export function parseReadiness(response){
 if(!response||![200,503].includes(response.status)||typeof response.body!=='string'||response.contentType?.toLowerCase().replace(/\s/g,'')!=='application/json;charset=utf-8'||response.cacheControl!=='no-store')fail('APPLICATION_RESPONSE_INVALID');
 let r;try{r=boundedJSON(response.body,MAX_BODY);}catch{fail('APPLICATION_RESPONSE_INVALID');}
 if(!exact(r,'schemaVersion,protocolVersion,platformVersion,ready,services')||r.schemaVersion!=='1.0.0-draft.1'||r.protocolVersion!=='1'||r.platformVersion!=='0.1.0'||typeof r.ready!=='boolean'||!Array.isArray(r.services)||r.services.length!==5||r.services.some(s=>!exact(s,'name,status')||!serviceNames.includes(s.name)||!['ready','unavailable'].includes(s.status))||new Set(r.services.map(s=>s.name)).size!==5)fail('APPLICATION_RESPONSE_INVALID');
 const all=r.services.every(s=>s.status==='ready');if(r.ready!==all||(response.status===200)!==all||r.services.find(s=>s.name==='platform').status!==(all?'ready':'unavailable'))fail('APPLICATION_RESPONSE_INVALID');
 return {status:all?'ready':'unavailable',httpStatus:response.status,schemaVersion:r.schemaVersion,protocolVersion:r.protocolVersion,platformVersion:r.platformVersion,components:serviceNames.map(name=>({name,status:r.services.find(s=>s.name===name).status}))};
}
/** No DNS/proxy/redirect/cookie/auth: the only target is the exact observed loopback TCP binding. */
export function readLoopbackReadiness(binding,{timeout}){
 return new Promise((resolve,reject)=>{
  let done=false,timer;const finish=(error,value)=>{if(done)return;done=true;clearTimeout(timer);if(error)reject(new DiagnosticError(error));else resolve(value);};
  const req=request({host:'127.0.0.1',port:binding.port,method:'GET',path:'/api/v1/readiness',agent:false,headers:{host:`127.0.0.1:${binding.port}`,accept:'application/json','cache-control':'no-cache','connection':'close'}},res=>{
   const chunks=[];let bytes=0;res.on('data',chunk=>{bytes+=chunk.length;if(bytes>MAX_BODY){finish('APPLICATION_RESPONSE_INVALID');res.destroy();req.destroy();}else chunks.push(chunk);});
   res.on('aborted',()=>finish('APPLICATION_UNAVAILABLE'));res.on('error',()=>finish('APPLICATION_UNAVAILABLE'));res.on('end',()=>{
    if(res.headers['content-encoding']&&res.headers['content-encoding']!=='identity')return finish('APPLICATION_RESPONSE_INVALID');let body;try{body=new TextDecoder('utf-8',{fatal:true}).decode(Buffer.concat(chunks));}catch{return finish('APPLICATION_RESPONSE_INVALID');}
    finish(null,{status:res.statusCode,body,contentType:res.headers['content-type'],cacheControl:res.headers['cache-control']});
   });
  });
  timer=setTimeout(()=>{finish('APPLICATION_TIMEOUT');req.destroy();},timeout);req.on('error',()=>finish('APPLICATION_UNAVAILABLE'));req.end();
 });
}
export async function diagnoseApplication(input,{service,run=dockerRead,readHTTP=readLoopbackReadiness,clock=()=>performance.now(),observedClock=Date.now,getEnvironment=()=>process.env}={}){
 const target=validateTarget(input);if(typeof service!=='string'||!target.services.includes(service))fail('TARGET_INVALID');
 const deadline=clock()+TOTAL_MS;const remaining=()=>{const n=deadline-clock();if(n<=0)fail('ENGINE_TIMEOUT');return n;};
 const engineRun=async(args,o)=>{const timeout=Math.min(o?.timeout??8000,remaining());const out=await run(args,{...o,timeout});remaining();return out;};
 const local=await bindLocalEngine(engineRun,getEnvironment);
 const boundedRun=(args,o)=>local.run(args,o);
 const baseline=await diagnose(target,{run:boundedRun,clock,observedClock});
 const result=(status,observation)=>{remaining();return {...baseline,scope:'readonly-compose-and-bound-platform-readiness-observations',applicationReadiness:status,applicationObservation:observation,observedEndMs:observedClock()};};
 const s=baseline.services.find(s=>s.service===service);if(baseline.censusChanged||baseline.unexpectedContainers||s?.containers!==1||s.state!=='running')return result('unknown',{status:'not-observed',reason:'service-binding-unverified'});
 const expected={...s,project:target.project};const inspect=async()=>parsePortInspection(await boundedRun(['inspect','--format',portTemplate,s.id]),expected);
 const before=await inspect();if(!before)return result('unknown',{status:'not-observed',reason:'single-loopback-8080-binding-required'});
 await local.verify();
 const observedStartMs=observedClock(),http=await readHTTP(before,{timeout:Math.min(2000,remaining())});remaining();const observation=parseReadiness(http);
 const after=await inspect(),ids=parseIds(await boundedRun(['ps','--all','--no-trunc','--filter',`label=com.docker.compose.project=${target.project}`,'--format','{{.ID}}']));remaining();
 await local.verify();
 const known=baseline.services.filter(s=>s.containers===1).map(s=>s.id).sort();if(JSON.stringify(before)!==JSON.stringify(after)||known.join(',')!==ids.join(','))return {...result('unknown',{status:'not-observed',reason:'binding-or-census-changed'}),censusChanged:true,containerHealth:'unknown'};
 return result(observation.status,{...observation,endpoint:`http://127.0.0.1:${before.port}/api/v1/readiness`,service,containerId:s.id,imageConfigDigest:s.imageConfigDigest,binding:before,observedStartMs,observedEndMs:observedClock(),atomicSnapshot:false,engineLocality:'observed-local-transport-trusted-daemon',signedReleaseAuthenticity:'not-verified',realtimeAndTLS:'not-verified'});
}
