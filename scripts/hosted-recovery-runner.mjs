// SPDX-License-Identifier: Apache-2.0
// Fixed native-only recovery runner. No renderer-selected commands/scripts/env.
import {spawn} from 'node:child_process';
import {lstat,realpath} from 'node:fs/promises';
import {isAbsolute,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {withRenderRecoveryConnection} from '../vendor/deployment/src/pages-render.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const fail=code=>{throw Error(code);};
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
const safeCodes=new Set(['HOSTED_TOKEN_REQUIRED','HOSTED_TOKEN_INVALID','HOSTED_PERMISSION_REQUIRED','HOSTED_PAID_PLAN_REFUSED','HOSTED_RATE_LIMITED','HOSTED_CONNECTION_FAILED','HOSTED_REDIRECT_REFUSED','HOSTED_RESPONSE_INVALID','HOSTED_REMOTE_REJECTED','HOSTED_RESOURCE_CONFLICT','HOSTED_RECOVERY_REQUEST','HOSTED_RECOVERY_NETWORK_REQUIRED','HOSTED_RECOVERY_DATABASE_EXPIRED','HOSTED_RECOVERY_WRITERS_ACTIVE','HOSTED_RECOVERY_CONNECTION_INVALID','HOSTED_RECOVERY_TLS_REQUIRED','HOSTED_RECOVERY_OPERATOR_REQUIRED','HOSTED_RECOVERY_EXECUTION_FAILED','HOSTED_RECOVERY_SOURCE_CHANGED','HOSTED_RECOVERY_RESULT_INVALID','HOSTED_BACKUP_REQUEST_INVALID','HOSTED_BACKUP_OUTPUT_CHANGED','HOSTED_BACKUP_HELPER_FAILED','HOSTED_BACKUP_RESULT_INVALID']);
export function validateRecoveryInput(input){
 if(!input||typeof input!=='object'||Array.isArray(input)||Object.keys(input).sort().join(',')!=='confirmExternalWritersStopped,execution,request,sourceCidr,tokens'||input.confirmExternalWritersStopped!==true||typeof input.sourceCidr!=='string'||input.sourceCidr.length>64||!input.tokens||Object.keys(input.tokens).join(',')!=='render'||typeof input.tokens.render!=='string'||input.tokens.render.length<1||input.tokens.render.length>8192)fail('HOSTED_BACKUP_REQUEST_INVALID');
 const e=input.execution;if(!e||typeof e!=='object'||Object.keys(e).sort().join(',')!=='dockerEndpoint,dockerPath,helperName,image,jobBudgetBytes,jobId,keyBase64,maxDumpBytes,outputPath,workVolume'||!uuid(e.jobId)||e.helperName!=='exhibitos-hosted-backup-'+e.jobId||e.workVolume!=='exhibitos-hosted-backup-work-'+e.jobId||!/^sha256:[a-f0-9]{64}$/.test(e.image)||typeof e.keyBase64!=='string')fail('HOSTED_BACKUP_REQUEST_INVALID');
 if(!Number.isSafeInteger(e.jobBudgetBytes)||e.jobBudgetBytes<4*1024*1024)fail('HOSTED_BACKUP_REQUEST_INVALID');
 if(!Number.isSafeInteger(e.maxDumpBytes)||e.maxDumpBytes<1024*1024||e.maxDumpBytes>32*1024*1024*1024)fail('HOSTED_BACKUP_REQUEST_INVALID');
 const key=Buffer.from(e.keyBase64,'base64');try{if(key.length!==32||key.toString('base64')!==e.keyBase64)fail('HOSTED_BACKUP_REQUEST_INVALID');}finally{key.fill(0);}
 for(const path of [e.outputPath,e.dockerPath])if(typeof path!=='string'||path.length>4096||!isAbsolute(path)||resolve(path)!==path||/[\x00-\x1f\x7f,]/.test(path))fail('HOSTED_BACKUP_REQUEST_INVALID');
 if(typeof e.dockerEndpoint!=='string'||!e.dockerEndpoint.startsWith('unix:///')||e.dockerEndpoint.length>4096||/[\x00-\x20\x7f?#]/.test(e.dockerEndpoint)||resolve(e.dockerEndpoint.slice(7))!==e.dockerEndpoint.slice(7))fail('HOSTED_BACKUP_REQUEST_INVALID');
 return input;
}
export function helperCommand(e){
 return {command:e.dockerPath,args:['run','--pull=never','--name',e.helperName,'--label','com.exhibitos.hosted-backup='+e.jobId,'--network','bridge','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--pids-limit','128','--memory','1536m','--user','0:0','--env','TMPDIR=/work','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--tmpfs','/run/exhibitos:rw,nosuid,nodev,size=1m,mode=0700','--mount','type=volume,source='+e.workVolume+',target=/work,volume-nocopy','--mount','type=bind,source='+e.outputPath+',target=/output','-i','--entrypoint','node',e.image,'--disable-proto=throw','--no-warnings','scripts/remote-backup-entry.mjs'],env:{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DOCKER_HOST:e.dockerEndpoint}};
}
async function outputPin(path){const named=await lstat(path,{bigint:true});if(!named.isDirectory()||named.isSymbolicLink()||(named.mode&0o077n)!==0n||await realpath(path)!==path)fail('HOSTED_BACKUP_OUTPUT_CHANGED');return {dev:named.dev,ino:named.ino,mode:named.mode,uid:named.uid};}
const samePin=(a,b)=>['dev','ino','mode','uid'].every(k=>a[k]===b[k]);
async function executeHelper(execution,connection,spawner=spawn){
 const command=helperCommand(execution),child=spawner(command.command,command.args,{env:command.env,stdio:['pipe','pipe','pipe']});
 let size=0,errorSize=0,failed=false;const chunks=[];let input=Buffer.from(JSON.stringify({databaseURL:connection.url,keyBase64:execution.keyBase64,maxDumpBytes:execution.maxDumpBytes,jobBudgetBytes:execution.jobBudgetBytes}));
 return new Promise((resolve,reject)=>{
  const refuse=()=>{failed=true;};
  child.stdout.on('data',chunk=>{size+=chunk.length;if(size>262144)refuse();else chunks.push(chunk);});
  child.stderr.on('data',chunk=>{errorSize+=chunk.length;if(errorSize>4096)refuse();});
  child.stdin.on('error',refuse);child.on('error',()=>{input.fill(0);reject(Error('HOSTED_BACKUP_HELPER_FAILED'));});
  child.stdin.end(input,()=>{input.fill(0);input=Buffer.alloc(0);});
  child.once('close',(code,signal)=>{input.fill(0);if(failed||code!==0||signal){reject(Error('HOSTED_BACKUP_HELPER_FAILED'));return;}try{const value=boundedJSON(Buffer.concat(chunks).toString('utf8'),262144);if(value?.errorCode)fail('HOSTED_BACKUP_HELPER_FAILED');resolve(value);}catch{reject(Error('HOSTED_BACKUP_RESULT_INVALID'));}finally{for(const chunk of chunks)chunk.fill(0);}});
 });
}
export async function runHostedRecovery(input,dependencies={}){
 try{
  input=validateRecoveryInput(input);const e=input.execution,pin=await outputPin(e.outputPath);
  // This acknowledgement is native-created from the same retained job/action.
  // It is not accepted as public renderer/remote operation input. Suspension and
  // resource binding are still independently observed by the provider bridge.
  const result=await (dependencies.bridge??withRenderRecoveryConnection)(input.request,{tokens:input.tokens,sourceCidr:input.sourceCidr,...(dependencies.fetch?{fetch:dependencies.fetch}:{}),confirmExternalWritersStopped:async()=>input.confirmExternalWritersStopped===true},async connection=>{
   if(!samePin(pin,await outputPin(e.outputPath)))fail('HOSTED_BACKUP_OUTPUT_CHANGED');
   const result=await (dependencies.executeHelper??((execution,value)=>executeHelper(execution,value,dependencies.spawn??spawn)))(e,connection);
   if(!samePin(pin,await outputPin(e.outputPath)))fail('HOSTED_BACKUP_OUTPUT_CHANGED');return result;
  });
  return result;
 }catch(error){const code=safeCodes.has(error?.message)?error.message:'HOSTED_BACKUP_HELPER_FAILED';throw Error(code);}
 finally{if(input?.tokens)input.tokens.render='';if(input?.execution)input.execution.keyBase64='';}
}
export async function readRecoveryInput(stream){const chunks=[];let size=0;for await(const part of stream){const b=Buffer.from(part);size+=b.length;if(size>131072)fail('HOSTED_BACKUP_REQUEST_INVALID');chunks.push(b);}try{return boundedJSON(Buffer.concat(chunks).toString('utf8'),131072);}finally{for(const b of chunks)b.fill(0);}}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{const result=await runHostedRecovery(await readRecoveryInput(process.stdin));process.stdout.write(JSON.stringify(result));}catch(error){const errorCode=safeCodes.has(error?.message)?error.message:'HOSTED_BACKUP_HELPER_FAILED';process.stdout.write(JSON.stringify({errorCode}));process.exitCode=1;}
}
