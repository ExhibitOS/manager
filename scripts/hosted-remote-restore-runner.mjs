// SPDX-License-Identifier: Apache-2.0
// Native-only, hash-pinned purpose runner. No renderer executable/URL/role inputs.
import {constants} from 'node:fs';
import {spawn} from 'node:child_process';
import {lstat,realpath,readFile,open,rename} from 'node:fs/promises';
import {createHash,randomUUID} from 'node:crypto';
import {isAbsolute,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {renderTransport,eligibleTarget,listTargets,prepareConnection,connectRestored,validateSourceCidr,admitCreatedNetwork} from './render-restore-provider.mjs';
import {createFreePostgres,validateRecoveryConfiguration} from '../vendor/deployment/src/pages-render.mjs';
const fail=c=>{throw Error(c);};
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
const sha=b=>createHash('sha256').update(b).digest('hex');
const path=p=>typeof p==='string'&&isAbsolute(p)&&resolve(p)===p&&p.length<=4096&&!/[\x00-\x1f\x7f,]/.test(p);
function shape(v,keys){return v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===keys.split(',').sort().join(',');}
export async function runChild(command,args,env,input,spawner=spawn){
 const child=spawner(command,args,{env,stdio:['pipe','pipe','pipe']});const chunks=[];let total=0,errors=0,failed=false;const bytes=input===undefined?Buffer.alloc(0):Buffer.from(JSON.stringify(input));
 return new Promise((resolve,reject)=>{
  child.stdout.on('data',b=>{total+=b.length;if(total<=32768)chunks.push(Buffer.from(b));else failed=true;});child.stderr.on('data',b=>{errors+=b.length;if(errors>4096)failed=true;});
  child.stdin.on('error',()=>{failed=true;});child.on('error',()=>{bytes.fill(0);reject(Error('HOSTED_REMOTE_RESTORE_FAILED'));});child.stdin.end(bytes,()=>bytes.fill(0));
  child.once('close',(code,signal)=>{bytes.fill(0);try{if(failed||code!==0||signal)fail('HOSTED_REMOTE_RESTORE_FAILED');resolve(input===undefined?Buffer.concat(chunks).toString().trim():JSON.parse(Buffer.concat(chunks)));}catch{reject(Error('HOSTED_REMOTE_RESTORE_FAILED'));}finally{for(const b of chunks)b.fill(0);}});
 });
}
async function privateCheckpoint(name){
 let before;try{before=await lstat(name);}catch(error){if(error.code==='ENOENT')return null;throw error;}
 if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1||before.uid!==process.getuid()||(before.mode&0o077)||before.size<1||before.size>16384)fail('HOSTED_RECOVERY_SOURCE_CHANGED');
 const f=await open(name,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);try{const opened=await f.stat();if(opened.dev!==before.dev||opened.ino!==before.ino)fail('HOSTED_RECOVERY_SOURCE_CHANGED');const bytes=await f.readFile(),after=await f.stat(),named=await lstat(name);if(bytes.length!==before.size||after.size!==before.size||after.mtimeMs!==before.mtimeMs||after.ctimeMs!==before.ctimeMs||named.dev!==before.dev||named.ino!==before.ino)fail('HOSTED_RECOVERY_SOURCE_CHANGED');return JSON.parse(bytes);}finally{await f.close();}
}
async function stage(e,o,phase,errorCode,network){
 const root=resolve(e.sourcePath,'../../..'),folder=root+'/hosted-remote-restoration-'+o.id;
 const st=await lstat(folder);if(!st.isDirectory()||st.isSymbolicLink()||await realpath(folder)!==folder||st.uid!==process.getuid()||(st.mode&0o077))fail('HOSTED_RECOVERY_SOURCE_CHANGED');
 const existing=JSON.parse(await readFile(folder+'/operation.json','utf8'));if(existing.id!==o.id||existing.sourceJobId!==o.sourceJobId||existing.expectedManifestSha256!==o.expectedManifestSha256||existing.target.postgresId!==o.target.postgresId&&!(existing.target.createRequested&&existing.target.postgresId===null))fail('HOSTED_RECOVERY_SOURCE_CHANGED');
 const prior=await privateCheckpoint(folder+'/runner-state.json');if(prior&&(prior.operationId!==o.id||prior.sourceJobId!==o.sourceJobId))fail('HOSTED_RECOVERY_SOURCE_CHANGED');const remembered=prior?.targetPostgresId===o.target.postgresId?{...(prior.creationProof?{creationProof:prior.creationProof}:{}),...(prior.networkAttempted?{networkAttempted:true,networkCidr:prior.networkCidr}:{})}:{};
 const temporary=folder+'/.runner-'+randomUUID()+'.json',f=await open(temporary,'wx',0o600);try{await f.writeFile(JSON.stringify({operationId:o.id,sourceJobId:o.sourceJobId,targetPostgresId:o.target.postgresId,phase,...remembered,...network,...(errorCode?{errorCode}:{})}));await f.sync();}finally{await f.close();}await rename(temporary,folder+'/runner-state.json');
 const directory=await open(folder,constants.O_RDONLY|constants.O_DIRECTORY|constants.O_NOFOLLOW);try{const opened=await directory.stat(),named=await lstat(folder);if(!opened.isDirectory()||opened.dev!==st.dev||opened.ino!==st.ino||opened.uid!==process.getuid()||(opened.mode&0o077)||!named.isDirectory()||named.isSymbolicLink()||named.dev!==opened.dev||named.ino!==opened.ino)fail('HOSTED_RECOVERY_SOURCE_CHANGED');await directory.sync();}finally{await directory.close();}
 return {operationId:o.id,sourceJobId:o.sourceJobId,targetPostgresId:o.target.postgresId,phase,...remembered,...network};
}
async function connectionStage(e,o,checkpoint){
 const root=resolve(e.sourcePath,'../../..'),folder=root+'/hosted-remote-restoration-'+o.id;
 const st=await lstat(folder);if(!st.isDirectory()||st.isSymbolicLink()||await realpath(folder)!==folder||st.uid!==process.getuid()||(st.mode&0o077))fail('HOSTED_RECOVERY_SOURCE_CHANGED');
 const name=folder+'/connection-state.json';
 if(checkpoint===undefined){try{const stat=await lstat(name);if(!stat.isFile()||stat.isSymbolicLink()||stat.nlink!==1||stat.size>16384)fail('HOSTED_RECOVERY_SOURCE_CHANGED');const v=JSON.parse(await readFile(name));if(v.operationId!==o.id)fail('HOSTED_RECOVERY_SOURCE_CHANGED');return v;}catch(error){if(error.code==='ENOENT')return null;throw error;}}
 const temporary=folder+'/.connection-'+randomUUID()+'.json',f=await open(temporary,'wx',0o600);try{await f.writeFile(JSON.stringify(checkpoint));await f.sync();}finally{await f.close();}await rename(temporary,name);
}
async function runConnection(input,api,dependencies){
 const {operation:o,execution:e}=input;
 if(!uuid(o.id)||!uuid(o.sourceJobId)||!o.target.postgresId||!/^sha256:[a-f0-9]{64}$/.test(e?.image)||![e?.sourcePath,e?.dockerPath,e?.entryPath].every(path)||!e.entryPath.endsWith('/scripts/remote-target-entry.mjs')||!/^unix:\/\//.test(e.dockerEndpoint)||!path(e.dockerEndpoint.slice(7))||/[ ?#]/.test(e.dockerEndpoint))fail('HOSTED_RECOVERY_REQUEST');
 const stat=await lstat(e.entryPath);if(!stat.isFile()||stat.isSymbolicLink()||stat.nlink!==1||await realpath(e.entryPath)!==e.entryPath||sha(await readFile(e.entryPath))!==e.entrySha256)fail('REMOTE_RUNTIME_CHANGED');
 if(input.action==='connection-plan'){
  if(!shape(input,'action,execution,expectedConfiguration,operation,sourceCidr,token'))fail('HOSTED_RECOVERY_REQUEST');
  return await prepareConnection(api,o,input.sourceCidr,input.expectedConfiguration);
 }
 if(!shape(input,'action,confirmServiceConnection,execution,operation,plan,token')||input.action!=='connect'||input.confirmServiceConnection!==true)fail('HOSTED_RECOVERY_REQUEST');
 const checkpoint=await connectionStage(e,o);
 const identity=async(databaseURL,expectedIdentitySha256,phase)=>{
  const helper='exhibitos-hosted-remote-identity-'+o.id+'-'+phase+'-'+randomUUID();
  const args=['run','--pull=never','--name',helper,'--label','com.exhibitos.hosted-remote='+o.id,'--network','bridge','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--pids-limit','64','--memory','256m','--user','0:0','--tmpfs','/tmp:rw,nosuid,nodev,size=16m,mode=1777','--mount','type=bind,source='+e.entryPath+',target=/opt/exhibitos/scripts/manager-remote-target-entry.mjs,readonly','-i','--entrypoint','node',e.image,'--disable-proto=throw','--no-warnings','/opt/exhibitos/scripts/manager-remote-target-entry.mjs'];
  const result=await (dependencies.runChild??runChild)(e.dockerPath,args,{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DOCKER_HOST:e.dockerEndpoint},{action:'identity',databaseURL,expectedIdentitySha256},dependencies.spawn);
  if(!shape(result,'operation,targetIdentitySha256')||result.operation!=='identity-verified'||result.targetIdentitySha256!==expectedIdentitySha256)fail('HOSTED_RECOVERY_RESULT_INVALID');
 };
 return await connectRestored(api,o,input.plan,checkpoint,value=>connectionStage(e,o,value),identity,dependencies);
}

export async function remoteRestore(input,dependencies={}){
 let checkpointPhase;
 try{
  const api=renderTransport(input.token,dependencies.fetch);
  if(['connection-plan','connect'].includes(input.action))return await runConnection(input,api,dependencies);
  if(input.action==='targets'){
   if(!shape(input,'action,source,token'))fail('HOSTED_RECOVERY_REQUEST');return {targets:await listTargets(api,input.source)};
  }
  if(!shape(input,'action,confirmExternalWritersStopped,execution,expectedConfiguration,operation,sourceCidr,token')||input.action!=='restore'||input.confirmExternalWritersStopped!==true)fail('HOSTED_RECOVERY_REQUEST');
  const o=input.operation,e=input.execution;
  if(!uuid(o.id)||!uuid(o.sourceJobId)||o.sourceJobId===o.id||!uuid(o.expectedBackupId)||!/^[a-f0-9]{64}$/.test(o.expectedManifestSha256)||!Number.isSafeInteger(o.expectedFiles)||o.expectedFiles<1||o.expectedFiles>100000||!shape(e,'dockerEndpoint,dockerPath,entryPath,entrySha256,image,jobBudgetBytes,keyBase64,sourcePath,workVolume')||![e.dockerPath,e.entryPath,e.sourcePath].every(path)||!e.entryPath.endsWith('/scripts/remote-target-entry.mjs')||!Number.isSafeInteger(e.jobBudgetBytes)||e.jobBudgetBytes<4194304||e.jobBudgetBytes>32*1024**3||e.workVolume!=='exhibitos-hosted-remote-work-'+o.id||!/^sha256:[a-f0-9]{64}$/.test(e.image)||!/^unix:\/\//.test(e.dockerEndpoint)||!path(e.dockerEndpoint.slice(7))||/[ ?#]/.test(e.dockerEndpoint))fail('HOSTED_RECOVERY_REQUEST');
  const key=Buffer.from(e.keyBase64,'base64');try{if(key.length!==32||key.toString('base64')!==e.keyBase64)fail('HOSTED_RECOVERY_REQUEST');}finally{key.fill(0);}
  const entry=await lstat(e.entryPath);if(!entry.isFile()||entry.isSymbolicLink()||entry.nlink!==1||await realpath(e.entryPath)!==e.entryPath||sha(await readFile(e.entryPath))!==e.entrySha256)fail('REMOTE_RUNTIME_CHANGED');
  const source=await lstat(e.sourcePath);if(!source.isDirectory()||source.isSymbolicLink()||await realpath(e.sourcePath)!==e.sourcePath)fail('HOSTED_RECOVERY_SOURCE_CHANGED');
  validateSourceCidr(input.sourceCidr);await stage(e,o,'admitting');checkpointPhase='admitting';
  if(o.target.postgresId===null){
   if(o.target.createRequested!==true||o.target.workspaceId!==o.source.workspaceId)fail('HOSTED_RECOVERY_REQUEST');
   const candidate=await createFreePostgres({workspaceId:o.target.workspaceId,projectName:'exhibitos-restore-'+o.id},{tokens:{render:input.token},...(dependencies.fetch?{fetch:dependencies.fetch}:{}),onPostgresCreated:async created=>{
    if(!shape(created,'id,ownerId,name')||created.ownerId!==o.source.workspaceId||created.name!=='exhibitos-restore-'+o.id+'-db'||typeof created.id!=='string'||!/^[a-zA-Z0-9_-]{1,128}$/.test(created.id)||created.id===o.source.postgresId)fail('HOSTED_RESULT_UNCERTAIN');
    o.target.postgresId=created.id;
    await stage(e,o,'admitting',undefined,{creationProof:{operationId:o.id,sourceJobId:o.sourceJobId,postgresId:created.id,ownerId:created.ownerId,name:created.name}});
   }});
   o.target.postgresId=candidate.id;await stage(e,o,'admitting');
   if(candidate.status!=='available')fail('HOSTED_RECOVERY_TARGET_NOT_READY');
  }
  if(o.target.createRequested){const checkpoint=await stage(e,o,'admitting');await admitCreatedNetwork(api,o,input.sourceCidr,checkpoint,network=>stage(e,o,'admitting',undefined,network));}
  const admitted=await eligibleTarget(api,o,input.sourceCidr,input.expectedConfiguration);
  const authority=validateRecoveryConfiguration(input.expectedConfiguration);
  // The core persisted 'dispatched' before invoking us. Even a response loss or
  // failed helper leaves this same operation recovery-required, never re-import.
  const childRun=dependencies.runChild??runChild,childEnv={PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DOCKER_HOST:e.dockerEndpoint};
  const prior=await childRun(e.dockerPath,['volume','ls','--filter','name=^'+e.workVolume+'$','--format','{{.Name}}'],childEnv,undefined,dependencies.spawn);
  if(prior!=='')fail('HOSTED_RECOVERY_HELPER_ACTIVE');
  const made=await childRun(e.dockerPath,['volume','create','--label','com.exhibitos.hosted-remote='+o.id,e.workVolume],childEnv,undefined,dependencies.spawn);
  if(made!==e.workVolume)fail('HOSTED_RECOVERY_HELPER_ACTIVE');
  const labels=JSON.parse(await childRun(e.dockerPath,['volume','inspect','--format','{{json .Labels}}',e.workVolume],childEnv,undefined,dependencies.spawn));
  if(labels?.['com.exhibitos.hosted-remote']!==o.id)fail('HOSTED_RECOVERY_HELPER_ACTIVE');
  const args=['run','--pull=never','--name','exhibitos-hosted-remote-'+o.id,'--label','com.exhibitos.hosted-remote='+o.id,'--network','bridge','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges:true','--pids-limit','128','--memory','1536m','--user','0:0','--env','TMPDIR=/work','--tmpfs','/tmp:rw,nosuid,nodev,size=256m,mode=1777','--tmpfs','/run/exhibitos:rw,nosuid,nodev,size=1m,mode=0700','--tmpfs','/var/lib/postgresql:rw,nosuid,nodev,size=1m,mode=0700','--mount','type=volume,source='+e.workVolume+',target=/work,volume-nocopy','--mount','type=bind,source='+e.sourcePath+',target=/source,readonly','--mount','type=bind,source='+e.entryPath+',target=/opt/exhibitos/scripts/manager-remote-target-entry.mjs,readonly','-i','--entrypoint','node',e.image,'--disable-proto=throw','--no-warnings','/opt/exhibitos/scripts/manager-remote-target-entry.mjs'];
  await stage(e,o,'importing');checkpointPhase='importing';
  const result=await (dependencies.runChild??runChild)(e.dockerPath,args,{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DOCKER_HOST:e.dockerEndpoint},{operationId:o.id,jobBudgetBytes:e.jobBudgetBytes,backupId:o.expectedBackupId,authenticatedManifestSha256:o.expectedManifestSha256,files:o.expectedFiles,databaseURL:admitted.connection.external,keyBase64:e.keyBase64,expectedAuthority:authority},dependencies.spawn);
  if(!shape(result,'authenticatedManifestSha256,backupId,files,operation,targetIdentitySha256')||result.operation!=='restored-and-verified'||result.backupId!==o.expectedBackupId||result.authenticatedManifestSha256!==o.expectedManifestSha256||result.files!==o.expectedFiles||!/^[a-f0-9]{64}$/.test(result.targetIdentitySha256))fail('HOSTED_RECOVERY_RESULT_INVALID');
  // Source identity/config must remain unchanged after SQL. A failure preserves
  // both DBs and refuses completion; it never switches or resumes the service.
  const after=await eligibleTarget(api,o,input.sourceCidr,input.expectedConfiguration);
  if(after.connection.internal!==admitted.connection.internal||JSON.stringify(after.originalEnvironment)!==JSON.stringify(admitted.originalEnvironment))fail('HOSTED_RECOVERY_SOURCE_CHANGED');
  await stage(e,o,'verified');
  return {state:'verified',operationId:o.id,sourceJobId:o.sourceJobId,backupId:o.expectedBackupId,authenticatedManifestSha256:o.expectedManifestSha256,files:o.expectedFiles,targetPostgresId:o.target.postgresId,serviceConnected:false,targetIdentitySha256:result.targetIdentitySha256};
 }catch(error){const code=error?.message,safe=typeof code==='string'&&/^(HOSTED_[A-Z_]{1,100}|REMOTE_RUNTIME_CHANGED)$/.test(code)?code:'HOSTED_REMOTE_RESTORE_FAILED';if(checkpointPhase)try{await stage(input.execution,input.operation,checkpointPhase,safe);}catch{}throw Error(safe);}
 finally{if(input){input.token='';if(input.execution)input.execution.keyBase64='';for(const config of [input.expectedConfiguration,input.plan?.expectedConfiguration])if(config?.envVars)for(const e of config.envVars)if(['ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'].includes(e.key))e.value='';if(input.plan){input.plan.targetDatabaseURL='';for(const e of input.plan.originalEnvironment??[])e.value='';}}}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){try{const chunks=[];let total=0;for await(const c of process.stdin){total+=c.length;if(total>131072)fail('HOSTED_RECOVERY_REQUEST');chunks.push(c);}const b=Buffer.concat(chunks);let input;try{input=JSON.parse(b);}finally{b.fill(0);for(const c of chunks)c.fill(0);}process.stdout.write(JSON.stringify(await remoteRestore(input)));}catch(error){process.stdout.write(JSON.stringify({errorCode:/^[A-Z_]+$/.test(error?.message)?error.message:'HOSTED_REMOTE_RESTORE_FAILED'}));process.exitCode=1;}}
