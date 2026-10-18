// SPDX-License-Identifier: Apache-2.0
// Native-only isolated DB restoration. No provider access or service activation.
import {spawn} from 'node:child_process';
import {lstat,realpath} from 'node:fs/promises';
import {isAbsolute,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {validateRecoveryConfiguration} from '../vendor/deployment/src/pages-render.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const fail=()=>{throw Error('HOSTED_RESTORE_CHECK_FAILED');};
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v)&&v!=='00000000-0000-0000-0000-000000000000';
const digest=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const closed=(v,fields)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===fields.split(',').sort().join(',');
const path=v=>typeof v==='string'&&v.length<=4096&&isAbsolute(v)&&resolve(v)===v&&!Array.from(v).some(c=>c.charCodeAt(0)<32||c.charCodeAt(0)===127||c===',');
export function validateRestoreInput(input){
 if(!closed(input,'execution,expectedConfiguration')&&!closed(input,'execution'))fail();
 if(Object.hasOwn(input,'expectedConfiguration'))validateRecoveryConfiguration(input.expectedConfiguration);
 const e=input.execution;
 if(!closed(e,'jobId,sourceJobId,helperName,image,sourcePath,workVolume,dockerPath,dockerEndpoint,jobBudgetBytes,expectedBackupId,expectedManifestSha256,expectedFiles,keyBase64')||!uuid(e.jobId)||!uuid(e.sourceJobId)||!uuid(e.expectedBackupId)||!digest(e.expectedManifestSha256)||e.helperName!=='exhibitos-hosted-restore-'+e.jobId||e.workVolume!=='exhibitos-hosted-restore-work-'+e.jobId||typeof e.image!=='string'||!/^sha256:[a-f0-9]{64}$/.test(e.image)||!path(e.sourcePath)||!path(e.dockerPath)||!Number.isSafeInteger(e.jobBudgetBytes)||e.jobBudgetBytes<4194304||e.jobBudgetBytes>32*1024**3||!Number.isSafeInteger(e.expectedFiles)||e.expectedFiles<1||e.expectedFiles>100000||typeof e.keyBase64!=='string')fail();
 if(typeof e.dockerEndpoint!=='string'||!e.dockerEndpoint.startsWith('unix:///')||!path(e.dockerEndpoint.slice(7))||/[ ?#]/.test(e.dockerEndpoint))fail();
 const key=Buffer.from(e.keyBase64,'base64');try{if(key.length!==32||key.toString('base64')!==e.keyBase64)fail();}finally{key.fill(0);}
 return input;
}
export function restoreHelperCommand(e){return {command:e.dockerPath,args:['run','--pull=never','--name',e.helperName,'--label','com.exhibitos.hosted-restore='+e.jobId,'--network','none','--read-only','--cap-drop','ALL','--cap-add','CHOWN','--cap-add','SETUID','--cap-add','SETGID','--cap-add','DAC_READ_SEARCH','--cap-add','KILL','--security-opt','no-new-privileges:true','--pids-limit','128','--memory','1536m','--user','0:0','--env','TMPDIR=/work','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--tmpfs','/run/exhibitos:rw,nosuid,nodev,size=1m,mode=0700','--tmpfs','/var/lib/postgresql:rw,nosuid,nodev,size=1m,mode=0700','--mount','type=volume,source='+e.workVolume+',target=/work,volume-nocopy','--mount','type=bind,source='+e.sourcePath+',target=/source,readonly','-i','--entrypoint','node',e.image,'--disable-proto=throw','--no-warnings','scripts/remote-restore-entry.mjs'],env:{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DOCKER_HOST:e.dockerEndpoint}};}
async function sourcePin(path){const s=await lstat(path,{bigint:true});if(!s.isDirectory()||s.isSymbolicLink()||(s.mode&0o077n)!==0n||await realpath(path)!==path)fail();return {dev:s.dev,ino:s.ino,mode:s.mode,uid:s.uid};}
const same=(a,b)=>['dev','ino','mode','uid'].every(k=>a[k]===b[k]);
async function execute(e,authority,spawner){
 const command=restoreHelperCommand(e),child=spawner(command.command,command.args,{env:command.env,stdio:['pipe','pipe','pipe']});
 let input=Buffer.from(JSON.stringify({jobId:e.jobId,keyBase64:e.keyBase64,expectedAuthority:authority,backupId:e.expectedBackupId,authenticatedManifestSha256:e.expectedManifestSha256,jobBudgetBytes:e.jobBudgetBytes}));
 let size=0,errorSize=0,failed=false;const chunks=[];
 return new Promise((resolve,reject)=>{
  child.stdout.on('data',b=>{size+=b.length;if(size>16384)failed=true;else chunks.push(Buffer.from(b));});
  child.stderr.on('data',b=>{errorSize+=b.length;if(errorSize>4096)failed=true;});
  child.stdin.on('error',()=>{failed=true;});child.on('error',()=>{input.fill(0);reject(Error('HOSTED_RESTORE_CHECK_FAILED'));});
  child.stdin.end(input,()=>{input.fill(0);input=Buffer.alloc(0);});
  child.once('close',(code,signal)=>{input.fill(0);try{if(failed||code!==0||signal)fail();resolve(boundedJSON(Buffer.concat(chunks).toString('utf8'),16384));}catch{reject(Error('HOSTED_RESTORE_CHECK_FAILED'));}finally{for(const b of chunks)b.fill(0);}});
 });
}
export async function runHostedRestoreCheck(input,dependencies={}){
 try{
  validateRestoreInput(input);const e=input.execution,pin=await sourcePin(e.sourcePath),authority=Object.hasOwn(input,'expectedConfiguration')?validateRecoveryConfiguration(input.expectedConfiguration):undefined;
  const result=await execute(e,authority,dependencies.spawn??spawn);
  if(!same(pin,await sourcePin(e.sourcePath))||!closed(result,'operation,backupId,authenticatedManifestSha256,files')||result.operation!=='restored-and-verified'||result.backupId!==e.expectedBackupId||result.authenticatedManifestSha256!==e.expectedManifestSha256||result.files!==e.expectedFiles)fail();
  return result;
 }catch{throw Error('HOSTED_RESTORE_CHECK_FAILED');}
 finally{if(input?.execution)input.execution.keyBase64='';if(Array.isArray(input?.expectedConfiguration?.envVars))for(const e of input.expectedConfiguration.envVars)if(e&&['ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'].includes(e.key))e.value='';}
}
export async function readRestoreInput(stream){const chunks=[];let size=0;try{for await(const part of stream){const b=Buffer.from(part);size+=b.length;if(size>131072)fail();chunks.push(b);}return boundedJSON(Buffer.concat(chunks).toString('utf8'),131072);}finally{for(const b of chunks)b.fill(0);}}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){try{const result=await runHostedRestoreCheck(await readRestoreInput(process.stdin));process.stdout.write(JSON.stringify(result));}catch{process.stdout.write('{"errorCode":"HOSTED_RESTORE_CHECK_FAILED"}');process.exitCode=1;}}
