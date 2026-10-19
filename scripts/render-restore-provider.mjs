// SPDX-License-Identifier: Apache-2.0
// Fixed native runner only. Provider credentials and URLs never enter renderer data.
import {isIP} from 'node:net';
import {validateRecoveryConfiguration} from '../vendor/deployment/src/pages-render.mjs';
const fail=c=>{throw Error(c);};
const id=v=>typeof v==='string'&&/^[a-zA-Z0-9_-]{1,128}$/.test(v);
export function renderTransport(token,fetcher=fetch){
 if(typeof token!=='string'||!token.length||token.length>8192||/[\r\n\0]/.test(token))fail('HOSTED_TOKEN_REQUIRED');
 return async(path,body,method=body===undefined?'GET':'PUT')=>{let r;try{r=await fetcher('https://api.render.com/v1'+path,{method,headers:{Authorization:'Bearer '+token,Accept:'application/json',...(body===undefined?{}:{'Content-Type':'application/json'})},redirect:'error',signal:AbortSignal.timeout(90000),...(body===undefined?{}:{body:JSON.stringify(body)})});}catch{fail(body===undefined?'HOSTED_CONNECTION_FAILED':'HOSTED_RESULT_UNCERTAIN');}
 if(!r.ok||r.redirected){await r.body?.cancel();fail(r.status===401?'HOSTED_TOKEN_INVALID':r.status===403?'HOSTED_PERMISSION_REQUIRED':body!==undefined?'HOSTED_RESULT_UNCERTAIN':'HOSTED_REMOTE_REJECTED');}
 if([202,204].includes(r.status)&&r.headers.get('content-length')==='0')return {};
 const reader=r.body?.getReader();if(!reader)return {};const chunks=[];let total=0;
 try{for(;;){const v=await reader.read();if(v.done)break;total+=v.value.length;if(total>1048576)fail('HOSTED_RESPONSE_INVALID');chunks.push(v.value);}}finally{await reader.cancel().catch(()=>{});}
 if(total===0&&[202,204].includes(r.status))return {};
 try{return JSON.parse(Buffer.concat(chunks).toString());}catch{fail('HOSTED_RESPONSE_INVALID');}
 };
}
export function providerExpiry(value){
 if(typeof value!=='string')fail('HOSTED_RESPONSE_INVALID');
 const m=/^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?([Zz]|[+-]\d{2}:\d{2})$/.exec(value);
 if(!m)fail('HOSTED_RESPONSE_INVALID');const [year,month,day,hour,minute,second]=m.slice(1,7).map(Number),leap=year%4===0&&(year%100!==0||year%400===0),days=[31,leap?29:28,31,30,31,30,31,31,30,31,30,31];
 if(month<1||month>12||day<1||day>days[month-1]||hour>23||minute>59||second>59||!['Z','z'].includes(m[7])&&(Number(m[7].slice(1,3))>23||Number(m[7].slice(4))>59))fail('HOSTED_RESPONSE_INVALID');
 const time=Date.parse(value);if(!Number.isFinite(time))fail('HOSTED_RESPONSE_INVALID');return time;
}
function owned(db,owner){if(!db||!id(db.id)||db.owner?.id!==owner||db.plan!=='free'||db.region!=='frankfurt'||db.version!=='18')fail('HOSTED_RESOURCE_CONFLICT');if(db.expiresAt!==undefined&&providerExpiry(db.expiresAt)<=Date.now())fail('HOSTED_RECOVERY_DATABASE_EXPIRED');return db;}
export function validateSourceCidr(value){const p=typeof value==='string'?value.split('/'):[];if(p.length!==2||!(isIP(p[0])===4&&p[1]==='32'||isIP(p[0])===6&&p[1]==='128'))fail('HOSTED_RECOVERY_NETWORK_REQUIRED');}
export function targetTLS(info){let u,i;try{u=new URL(info.externalConnectionString);i=new URL(info.internalConnectionString);}catch{fail('HOSTED_RECOVERY_CONNECTION_INVALID');}
 if(!['postgres:','postgresql:'].includes(u.protocol)||!['postgres:','postgresql:'].includes(i.protocol)||!u.username||!u.password||u.username!==i.username||u.password!==i.password||u.pathname!==i.pathname||u.hostname.split('.')[0]!==i.hostname||u.hash||u.port&&u.port!=='5432'||!/^[a-z0-9-]+\.[a-z0-9-]+-postgres\.render\.com$/.test(u.hostname)||[...u.searchParams].some(([k,v])=>k!=='sslmode'||!['require','verify-full'].includes(v))||u.searchParams.getAll('sslmode').length>1)fail('HOSTED_RECOVERY_CONNECTION_INVALID');
 u.search='';u.searchParams.set('sslmode','verify-full');return {external:u.href,internal:info.internalConnectionString};}
export function matchOriginal(rows,expected){
 validateRecoveryConfiguration(expected);if(!Array.isArray(rows)||rows.length>100)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 const keys=new Set(expected.envVars.map(e=>e.key)),map=new Map();for(const row of rows){const e=row?.envVar;if(!e||typeof e.key!=='string'||typeof e.value!=='string'||map.has(e.key))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');map.set(e.key,e.value);}
 if(!expected.envVars.every(e=>map.get(e.key)===e.value)||[...map.keys()].some(k=>k.startsWith('STATIC_')&&!keys.has(k)))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');return map;
}
export async function eligibleTarget(api,operation,sourceCidr,expectedConfiguration){
 validateSourceCidr(sourceCidr);const {source,target}=operation;
 if(![source.workspaceId,source.serviceId,source.postgresId,target.workspaceId,target.postgresId].every(id)||target.postgresId===source.postgresId)fail('HOSTED_RESOURCE_CONFLICT');
 const original=await api('/postgres/'+source.postgresId),service=await api('/services/'+source.serviceId);
 // An expired source need not be SQL-accessible, but its identity/owner is retained.
 if(original.id!==source.postgresId||original.owner?.id!==source.workspaceId||service.id!==source.serviceId||service.ownerId!==source.workspaceId||service.suspended!=='suspended'||service.type!=='web_service'||service.repo!=='https://github.com/ExhibitOS/platform'||service.serviceDetails?.runtime!=='node'||service.serviceDetails?.plan!=='free')fail('HOSTED_RECOVERY_WRITERS_ACTIVE');
 const rows=await api('/services/'+source.serviceId+'/env-vars?limit=100');const env=matchOriginal(rows,expectedConfiguration);
 let origin;try{const url=new URL(env.get('AUTH_ORIGIN')??env.get('RENDER_EXTERNAL_URL')??service.serviceDetails?.url);if(url.protocol!=='https:'||url.username||url.password||url.port||url.search||url.hash||url.pathname!=='/')fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');origin=url.origin;}catch{fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');}if(origin!==expectedConfiguration.origin)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 const db=owned(await api('/postgres/'+target.postgresId),target.workspaceId);
 if(db.id!==target.postgresId||db.status!=='available'||!Array.isArray(db.ipAllowList)||!db.ipAllowList.some(r=>r.cidrBlock===sourceCidr))fail('HOSTED_RECOVERY_NETWORK_REQUIRED');
 const connection=targetTLS(await api('/postgres/'+target.postgresId+'/connection-info'));
 const sourceInfo=await api('/postgres/'+source.postgresId+'/connection-info');if(typeof sourceInfo?.internalConnectionString!=='string'||env.get('DATABASE_URL')!==sourceInfo.internalConnectionString)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 if(env.get('DATABASE_URL')===connection.internal)fail('HOSTED_RESOURCE_CONFLICT');
 return {connection,originalEnvironment:canonicalEnv(rows),originalService:service};
}
export async function listTargets(api,source){
 // Existing workspace only. No account/workspace creation or implicit owner switch.
 let cursor;const result=[];
 for(let page=0;page<100;page++){
  const rows=await api('/postgres?limit=100&ownerId='+encodeURIComponent(source.workspaceId)+(cursor?'&cursor='+encodeURIComponent(cursor):''));if(!Array.isArray(rows))fail('HOSTED_RESPONSE_INVALID');
  for(const row of rows){const db=row?.postgres;if(!db||!id(db.id))fail('HOSTED_RESPONSE_INVALID');if(db.owner?.id!==source.workspaceId||db.id===source.postgresId)continue;try{owned(db,source.workspaceId);}catch{continue;}if(db.status==='available')result.push({workspaceId:source.workspaceId,postgresId:db.id,name:typeof db.name==='string'?db.name.replace(/[\x00-\x1f\x7f]/g,'').slice(0,128):db.id});}
  if(rows.length<100)return result;const next=rows.at(-1)?.cursor;if(!id(next)||next===cursor)fail('HOSTED_RESPONSE_INVALID');cursor=next;
 }fail('HOSTED_RESPONSE_INVALID');
}
// Provider operations below are native-purpose-only. Snapshot bytes stay in the
// immutable per-operation Keychain item, never a renderer or plaintext journal.
const digest=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const commit=v=>typeof v==='string'&&/^[a-f0-9]{40}$/.test(v);
function canonicalEnv(rows){if(!Array.isArray(rows)||rows.length>100)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');const seen=new Set();return rows.map(row=>{const e=row?.envVar??row;if(!e||typeof e.key!=='string'||!/^[A-Z][A-Z0-9_]{0,63}$/.test(e.key)||typeof e.value!=='string'||e.value.length>32768||seen.has(e.key))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');seen.add(e.key);return {key:e.key,value:e.value};}).sort((a,b)=>a.key.localeCompare(b.key));}
const sameEnv=(a,b)=>JSON.stringify(canonicalEnv(a))===JSON.stringify(canonicalEnv(b));
function desiredEnv(plan){return plan.originalEnvironment.map(e=>e.key==='DATABASE_URL'?{key:e.key,value:plan.targetDatabaseURL}:{...e});}
export async function prepareConnection(api,operation,sourceCidr,expectedConfiguration){
 if(operation.state!=='verified'||!digest(operation.targetIdentitySha256))fail('HOSTED_REMOTE_RECONCILIATION_REQUIRED');
 const checked=await eligibleTarget(api,operation,sourceCidr,expectedConfiguration),service=checked.originalService;
 if(!(service.autoDeployTrigger==='off'||service.autoDeploy==='no'||service.autoDeploy===false))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 const rows=await api('/services/'+service.id+'/deploys?limit=20');if(!Array.isArray(rows)||rows.length>20)fail('HOSTED_RESPONSE_INVALID');
 const deployed=rows.map(r=>r?.deploy).filter(d=>d?.status==='live'&&id(d.id)&&commit(d.commit?.id));if(!deployed.length)fail('HOSTED_RECOVERY_RUNTIME_UNAVAILABLE');
 const before=canonicalEnv(checked.originalEnvironment),sourceInfo=await api('/postgres/'+operation.source.postgresId+'/connection-info');
 if(typeof sourceInfo?.internalConnectionString!=='string'||before.find(e=>e.key==='DATABASE_URL')?.value!==sourceInfo.internalConnectionString)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 return {operationId:operation.id,sourceJobId:operation.sourceJobId,source:operation.source,target:operation.target,targetIdentitySha256:operation.targetIdentitySha256,originalEnvironment:before,targetDatabaseURL:checked.connection.internal,originalDeployId:deployed[0].id,originalCommitId:deployed[0].commit.id,knownDeployIds:rows.map(r=>r?.deploy?.id).filter(id),originalSuspended:true,origin:expectedConfiguration.origin,expectedConfiguration:structuredClone(expectedConfiguration),backupTimeRuntimeRecorded:false};
}
function validateConnectionPlan(plan,operation){
 if(!plan||plan.operationId!==operation.id||plan.sourceJobId!==operation.sourceJobId||plan.target?.postgresId!==operation.target.postgresId||plan.target?.workspaceId!==operation.target.workspaceId||plan.source?.serviceId!==operation.source.serviceId||plan.source?.postgresId!==operation.source.postgresId||plan.source?.workspaceId!==operation.source.workspaceId||plan.targetIdentitySha256!==operation.targetIdentitySha256||!commit(plan.originalCommitId)||!id(plan.originalDeployId)||typeof plan.targetDatabaseURL!=='string'||plan.originalSuspended!==true||!Array.isArray(plan.knownDeployIds)||plan.knownDeployIds.length>20||!plan.knownDeployIds.every(id)||plan.backupTimeRuntimeRecorded!==false)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 validateRecoveryConfiguration(plan.expectedConfiguration);matchOriginal(canonicalEnv(plan.originalEnvironment).map(envVar=>({envVar})),plan.expectedConfiguration);
 if(plan.origin!==plan.expectedConfiguration.origin||plan.originalEnvironment.filter(e=>e.key==='DATABASE_URL').length!==1)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
}
async function boundService(api,plan){const service=await api('/services/'+plan.source.serviceId);if(service.id!==plan.source.serviceId||service.ownerId!==plan.source.workspaceId||service.type!=='web_service'||service.repo!=='https://github.com/ExhibitOS/platform'||service.serviceDetails?.runtime!=='node'||service.serviceDetails?.plan!=='free'||!(service.autoDeployTrigger==='off'||service.autoDeploy==='no'||service.autoDeploy===false))fail('HOSTED_RESOURCE_CONFLICT');return service;}
async function currentEnv(api,plan){const rows=await api('/services/'+plan.source.serviceId+'/env-vars?limit=100');if(rows.length===100)fail('HOSTED_RESPONSE_INVALID');return canonicalEnv(rows);}
async function observeDeploy(api,serviceId,deployId,commitId,{delay=ms=>new Promise(r=>setTimeout(r,ms))}={}){
 for(let attempt=0;attempt<120;attempt++){
  const deploy=await api('/services/'+serviceId+'/deploys/'+deployId);if(deploy.id!==deployId||deploy.commit?.id!==commitId)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  if(deploy.status==='live')return deploy;
  if(['build_failed','update_failed','canceled','pre_deploy_failed','deactivated'].includes(deploy.status))fail('HOSTED_RECOVERY_HEALTH_FAILED');
  await delay(5000);
 }fail('HOSTED_RECOVERY_HEALTH_FAILED');
}
async function runtimeHealth(plan,{fetch:fetcher=fetch}={}){
 const get=async(path,options={})=>{let response;try{response=await fetcher(plan.origin+path,{...options,redirect:'error',signal:AbortSignal.timeout(30000)});}catch{fail('HOSTED_RECOVERY_HEALTH_FAILED');}if(!response.ok||response.redirected)fail('HOSTED_RECOVERY_HEALTH_FAILED');const reader=response.body?.getReader();if(!reader)fail('HOSTED_RECOVERY_HEALTH_FAILED');const chunks=[];let total=0;try{for(;;){const item=await reader.read();if(item.done)break;total+=item.value.length;if(total>1048576)fail('HOSTED_RECOVERY_HEALTH_FAILED');chunks.push(item.value);}}finally{await reader.cancel().catch(()=>{});}const text=Buffer.concat(chunks).toString();try{return {value:JSON.parse(text),headers:response.headers};}catch{fail('HOSTED_RECOVERY_HEALTH_FAILED');}};
 const health=(await get('/api/v1/readiness')).value;
 if(health?.ready!==true||health.protocolVersion!=='1'||!Array.isArray(health.services)||!['platform','api','database','web','storage'].every(name=>health.services.filter(s=>s.name===name&&s.status==='ready').length===1))fail('HOSTED_RECOVERY_HEALTH_FAILED');
 // All original SQL/blob/sequence/authority checks occurred before this login,
 // which legitimately writes a session into the restored destination DB.
 const env=new Map(plan.originalEnvironment.map(e=>[e.key,e.value])),headers={Origin:env.get('CLIENT_ORIGIN'),'Content-Type':'application/json'};
 const login=await get('/api/v1/auth/login',{method:'POST',headers,body:JSON.stringify({subject:env.get('ADMIN_SUBJECT'),password:env.get('ADMIN_PASSWORD'),tenantId:env.get('TENANT_ID')})});
 const cookie=login.headers.get('set-cookie')?.split(';',1)[0];if(!cookie)fail('HOSTED_RECOVERY_HEALTH_FAILED');
 const session=(await get('/api/v1/auth/session',{headers:{Origin:env.get('CLIENT_ORIGIN'),Cookie:cookie}})).value;if(session.tenantId!==env.get('TENANT_ID'))fail('HOSTED_RECOVERY_HEALTH_FAILED');
 if(env.has('STATIC_PUBLICATION_ID')){const publication=(await get('/api/v1/publications/'+encodeURIComponent(env.get('STATIC_PUBLICATION_ID')))).value;if(publication?.publication?.id!==env.get('STATIC_PUBLICATION_ID')||publication.publication.revisionSha256!==env.get('STATIC_REVISION_SHA256'))fail('HOSTED_RECOVERY_HEALTH_FAILED');}
 return true;
}
export async function connectRestored(api,operation,plan,checkpoint,save,identity,dependencies={}){
 validateConnectionPlan(plan,operation);if(typeof identity!=='function')fail('HOSTED_RECOVERY_REQUEST');
 const desired=desiredEnv(plan),route='/services/'+plan.source.serviceId;let mutationPossible=!!checkpoint&&!['connection-restored','connection-refused'].includes(checkpoint.phase);
 const phase=async(value,extra={})=>{checkpoint={...checkpoint,operationId:operation.id,phase:value,...extra};await save(checkpoint);};
 async function recover(){
  const service=await boundService(api,plan),env=await currentEnv(api,plan);
  if(!sameEnv(env,desired)&&!sameEnv(env,plan.originalEnvironment))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  if(checkpoint?.rollbackAttempted&&!checkpoint.rollbackDeployId)fail('HOSTED_RESULT_UNCERTAIN');
  await phase('restoring-original');
  // Suspend before reconnecting; preserve both DBs and all newly written target
  // data. Restore only the exact old connection/commit, never a current export.
  if(service.suspended!=='suspended')await api(route+'/suspend',{},'POST');
  if(!sameEnv(env,plan.originalEnvironment)){if(!sameEnv(await currentEnv(api,plan),desired))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');await api(route+'/env-vars',plan.originalEnvironment,'PUT');}
  if(!sameEnv(await currentEnv(api,plan),plan.originalEnvironment))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  // Restore the exact retained provider deploy as well as the old environment.
  // A lost rollback response is not replayed. Keep both DBs and pause in the
  // original service state if provider recovery cannot be verified.
  if(!checkpoint?.rollbackAttempted){
   await phase('rollback-dispatched',{rollbackAttempted:true});
   const rollback=await api(route+'/rollback',{deployId:plan.originalDeployId},'POST');
   if(!id(rollback?.id)||rollback.commit?.id!==plan.originalCommitId)fail('HOSTED_RESULT_UNCERTAIN');
   await phase('rollback-observing',{rollbackDeployId:rollback.id});
  }
  await observeDeploy(api,plan.source.serviceId,checkpoint.rollbackDeployId,plan.originalCommitId,dependencies);
  // Original service was suspended. Do not start an expired original DB or
  // claim old DB runtime health; its exact deploy and settings are retained.
  const observed=await api(route);if(observed.suspended!=='suspended')fail('HOSTED_RECOVERY_WRITERS_ACTIVE');
  const deploys=await api(route+'/deploys?limit=20');
  const active=deploys.find(r=>r.deploy?.status==='live')?.deploy;
  if(active?.commit?.id!==plan.originalCommitId)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  // The switch always deploys this exact same observed commit. If the provider
  // no longer identifies that commit as live, do not invent a successful return.
  if(!sameEnv(await currentEnv(api,plan),plan.originalEnvironment))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  await phase('connection-restored');return {state:'connection-restored',operationId:operation.id,sourceJobId:operation.sourceJobId,targetPostgresId:operation.target.postgresId,originalCommitId:plan.originalCommitId,serviceSuspended:true};
 }
 try{
  if(checkpoint&&checkpoint.operationId!==operation.id)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  if(checkpoint&&!['connection-restored','connection-refused'].includes(checkpoint.phase)){
   if(['observing-deploy','connected'].includes(checkpoint.phase)&&id(checkpoint.deployId)){
    try{await boundService(api,plan);if(!sameEnv(await currentEnv(api,plan),desired))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');await observeDeploy(api,plan.source.serviceId,checkpoint.deployId,plan.originalCommitId,dependencies);const info=targetTLS(await api('/postgres/'+operation.target.postgresId+'/connection-info'));if(info.internal!==plan.targetDatabaseURL)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');await identity(info.external,operation.targetIdentitySha256,'resumed');await runtimeHealth(plan,dependencies);await phase('connected');return {state:'connected',operationId:operation.id,sourceJobId:operation.sourceJobId,targetPostgresId:operation.target.postgresId,deployId:checkpoint.deployId,commitId:plan.originalCommitId,backupTimeRuntimeRecorded:false};}catch{return await recover();}
   }
   return await recover();
  }
  const service=await boundService(api,plan);if(service.suspended!=='suspended'||!sameEnv(await currentEnv(api,plan),plan.originalEnvironment))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  const target=owned(await api('/postgres/'+operation.target.postgresId),operation.target.workspaceId);if(target.id!==operation.target.postgresId||target.status!=='available')fail('HOSTED_RESOURCE_CONFLICT');
  const targetInfo=targetTLS(await api('/postgres/'+target.id+'/connection-info'));if(targetInfo.internal!==plan.targetDatabaseURL)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  await identity(targetInfo.external,operation.targetIdentitySha256,'before');
  const beforeDeploys=await api(route+'/deploys?limit=20');const current=beforeDeploys.find(r=>r.deploy?.status==='live')?.deploy;if(![plan.originalDeployId,...(checkpoint?.phase==='connection-restored'&&id(checkpoint.rollbackDeployId)?[checkpoint.rollbackDeployId]:[])].includes(current?.id)||current.commit?.id!==plan.originalCommitId)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  if(!sameEnv(await currentEnv(api,plan),plan.originalEnvironment))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  checkpoint=null;mutationPossible=true;await phase('switching');await api(route+'/env-vars',desired,'PUT');
  if(!sameEnv(await currentEnv(api,plan),desired))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  await phase('resuming');await api(route+'/resume',{},'POST');
  await phase('deploying');const deploy=await api(route+'/deploys',{commitId:plan.originalCommitId,clearCache:'do_not_clear'},'POST');
  if(!id(deploy?.id)||deploy.commit?.id!==plan.originalCommitId)fail('HOSTED_RESULT_UNCERTAIN');
  await phase('observing-deploy',{deployId:deploy.id});await observeDeploy(api,plan.source.serviceId,deploy.id,plan.originalCommitId,dependencies);
  await boundService(api,plan);if(!sameEnv(await currentEnv(api,plan),desired))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
  await identity(targetInfo.external,operation.targetIdentitySha256,'after');await runtimeHealth(plan,dependencies);
  await phase('connected',{deployId:deploy.id});return {state:'connected',operationId:operation.id,sourceJobId:operation.sourceJobId,targetPostgresId:operation.target.postgresId,deployId:deploy.id,commitId:plan.originalCommitId,backupTimeRuntimeRecorded:false};
 }catch{
  if(!mutationPossible){await phase('connection-refused');return {state:'connection-recovery-required',operationId:operation.id,sourceJobId:operation.sourceJobId,targetPostgresId:operation.target.postgresId};}
  try{return await recover();}catch{await phase('connection-recovery-required');return {state:'connection-recovery-required',operationId:operation.id,sourceJobId:operation.sourceJobId,targetPostgresId:operation.target.postgresId};}
 }
}

// Explicit new-target-only permission: never a selected existing DB or source.
// A durable dispatch marker makes every later attempt GET-only reconciliation.
export async function admitCreatedNetwork(api,operation,sourceCidr,checkpoint,markAttempt){
 validateSourceCidr(sourceCidr);const {target}=operation;
 if(target.createRequested!==true||target.workspaceId!==operation.source.workspaceId||target.postgresId===operation.source.postgresId||typeof markAttempt!=='function')fail('HOSTED_RESOURCE_CONFLICT');
 const route='/postgres/'+target.postgresId,check=db=>{
  owned(db,target.workspaceId);
  if(db.id!==target.postgresId||db.name!=='exhibitos-restore-'+operation.id+'-db'||db.status!=='available'||!Array.isArray(db.ipAllowList))fail('HOSTED_RESOURCE_CONFLICT');return db;
 };
 const exact=db=>db.ipAllowList.length===1&&db.ipAllowList[0].cidrBlock===sourceCidr;
 const before=check(await api(route));
 const proof=checkpoint?.creationProof,created=proof&&Object.keys(proof).sort().join(',')==='name,operationId,ownerId,postgresId,sourceJobId'&&proof.operationId===operation.id&&proof.sourceJobId===operation.sourceJobId&&proof.postgresId===before.id&&proof.ownerId===before.owner.id&&proof.name===before.name;
 if(checkpoint?.networkAttempted){if(!created)fail('HOSTED_RECOVERY_CREATION_UNPROVEN');}
 if(checkpoint?.networkAttempted){if(checkpoint.networkCidr!==sourceCidr)fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');if(exact(before))return;fail('HOSTED_RESULT_UNCERTAIN');}
 if(exact(before))return;
 if(before.ipAllowList.length!==0)fail('HOSTED_RECOVERY_NETWORK_REQUIRED');
 if(!created)fail('HOSTED_RECOVERY_CREATION_UNPROVEN');
 const latest=check(await api(route));
 const binding=db=>JSON.stringify([db.id,db.name,db.owner.id,db.plan,db.version,db.region,db.status,db.expiresAt??null,db.ipAllowList]);
 if(binding(latest)!==binding(before))fail('HOSTED_RECOVERY_CONFIGURATION_CHANGED');
 await markAttempt({networkAttempted:true,networkCidr:sourceCidr});
 // The user authorized one confirmed host rule on this operation's new DB.
 // A lost response is followed by GET only, including a future app restart.
 try{await api(route,{ipAllowList:[{cidrBlock:sourceCidr,description:'ExhibitOS explicit restore host'}]},'PATCH');}catch{}
 const after=check(await api(route));
 if(!exact(after)||JSON.stringify([after.id,after.name,after.owner.id,after.plan,after.version,after.region,after.expiresAt??null])!==JSON.stringify([before.id,before.name,before.owner.id,before.plan,before.version,before.region,before.expiresAt??null]))fail('HOSTED_RESULT_UNCERTAIN');
}
