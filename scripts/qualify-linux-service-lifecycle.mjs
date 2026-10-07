// SPDX-License-Identifier: Apache-2.0
// Trusted disposable CI qualification. Uses public Platform APIs/modules; never a release permit.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtemp, realpath, readFile, writeFile, readdir, lstat, cp} from 'node:fs/promises';
import {createReadStream, readFileSync} from 'node:fs';
import {createHash, randomBytes, randomUUID} from 'node:crypto';
import {join, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

let stage = 'preflight';
const checks = [];
let nativeJobFailure=null;
let lastHttpFailure=null;
let frozenReport=null;
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
async function hashed(path) {const h=createHash('sha256');for await(const b of createReadStream(path))h.update(b);return h.digest('hex');}
async function inventory(root) {
  const rows=[];
  async function walk(path, prefix='') {for(const n of (await readdir(path)).sort()) {const p=join(path,n),s=await lstat(p),name=prefix+n;assert(!s.isSymbolicLink());if(s.isDirectory())await walk(p,name+'/');else {assert(s.isFile()&&s.nlink===1);rows.push({name,bytes:s.size,mode:s.mode&0o777,sha256:await hashed(p)});}}}
  await walk(root);return rows;
}
function call(root, command, ...args) {
  const result=spawnSync(process.env.EXHIBITOS_MANAGER_CHECK_BINARY,['--root',root,command,...args],{encoding:'utf8',timeout:600000,maxBuffer:8*1024*1024});
  let value;try{value=JSON.parse(result.stdout);}catch{throw Error('MANAGER_JSON_INVALID');}
  if(result.status!==0){const code=value.code??value.errorCode;if(command==='restore-backup'){try{const job=JSON.parse(readFileSync(join(root,'restoration.json'),'utf8'));nativeJobFailure={state:/^(failed|interrupted)$/.test(job.state)?job.state:null,stage:/^[a-z][a-z-]{0,63}$/.test(job.stage)?job.stage:null,code:/^[A-Z][A-Z0-9_]{0,79}$/.test(job.errorCode??'')?job.errorCode:null};if(job.stage==='creating-fresh-target'){const m=JSON.parse(readFileSync(join(root,'bundle','manifest.json'),'utf8'));if(/^exhibitos-[0-9a-f-]{36}$/.test(m.projectName)){const inspected=spawnSync('docker',['network','inspect',m.projectName+'_default'],{encoding:'utf8',timeout:15000,maxBuffer:1024*1024});if(inspected.status===0){const n=JSON.parse(inspected.stdout)[0],configs=n.IPAM?.Config;nativeJobFailure.networkShape={labelsMatch:['bundle','project','schema'].every(k=>n.Labels?.['com.exhibitos.'+k]===m[{bundle:'bundleId',project:'projectName',schema:'schemaVersion'}[k]]),driverBridge:n.Driver==='bridge',ipv6Disabled:n.EnableIPv6===false,configCount:Array.isArray(configs)?configs.length:null,ipRangePresent:configs?.[0]?Object.hasOwn(configs[0],'IPRange'):false,ipRangeType:configs?.[0]?.IPRange===null?'null':typeof configs?.[0]?.IPRange,ipRangeEmpty:configs?.[0]?.IPRange==='',gatewayType:typeof configs?.[0]?.Gateway};}}}}catch{nativeJobFailure={code:'DIAGNOSTIC_NOT_AVAILABLE'};}}throw Error(/^[A-Z][A-Z0-9_]{0,79}$/.test(code??'')?code:'MANAGER_COMMAND_FAILED');}
  if(['install','start','stop','restart'].includes(command))assert.equal(value.state,'completed',value.errorCode??'MANAGER_JOB_NOT_COMPLETED');
  if(command==='create-backup'){assert.equal(value.operation,'created-and-authenticated','BACKUP_RECEIPT_INVALID');assert.equal(value.writersPaused,true,'BACKUP_WRITERS_NOT_PAUSED');const jobs=call(root,'backup-jobs');assert.equal(jobs.find(j=>j.id===value.id)?.state,'completed','BACKUP_JOB_NOT_COMPLETED');}
  if(command==='restore-backup'){assert.equal(value.operation,'restored-and-running','RESTORE_RECEIPT_INVALID');const job=call(root,'restoration-status');assert.equal(job?.state,'completed','RESTORE_JOB_NOT_COMPLETED');assert.equal(job?.id,value.id,'RESTORE_JOB_RECEIPT_MISMATCH');assert.equal(job?.stage,'complete','RESTORE_JOB_STAGE_INVALID');assert.equal(job?.errorCode,null,'RESTORE_JOB_ERROR_PRESENT');}return value;
}
async function ready(root) {const status=call(root,'status');assert.equal(status.readiness.ready,true);assert.equal(status.readiness.protocolVersion,'1');return status;}
async function client(origin, settings) {
  let cookie,csrf;
  const api=async(method,path,body,expected=200,extra={})=> {
    const response=await fetch(origin+path,{method,headers:{origin,connection:'close',...(cookie?{cookie,'x-csrf-token':csrf}:{}),...(body===undefined?{}:{'content-type':Buffer.isBuffer(body)?'application/octet-stream':'application/json'}),...extra},...(body===undefined?{}:{body:Buffer.isBuffer(body)?body:JSON.stringify(body)})});
    if(response.status!==expected){lastHttpFailure={method,status:response.status,expected,route:path.endsWith('/auth/login')?'auth-login':path.endsWith('/auth/session')?'auth-session':path.endsWith('/cms/artists')?'cms-artists':path.endsWith('/bytes')?'asset-bytes':path.endsWith('/freeze/authority')?'freeze-authority':'other-synthetic-api'};throw Error('RUNTIME_HTTP_STATUS');}return response;
  };
  const login=await api('POST','/api/v1/auth/login',{subject:settings.ADMIN_SUBJECT,password:settings.ADMIN_PASSWORD,tenantId:settings.TENANT_ID});cookie=login.headers.get('set-cookie').split(';')[0];csrf=(await(await api('GET','/api/v1/auth/session')).json()).csrfToken;
  return api;
}
try {
  assert.equal(process.env.EXHIBITOS_DISPOSABLE_LINUX_RUNTIME_CHECK,'1');assert.equal(process.platform,'linux');assert.notEqual(process.getuid(),0);
  const temp=await realpath(process.env.RUNNER_TEMP),platform=await realpath(process.env.EXHIBITOS_PUBLIC_PLATFORM_ROOT);
  assert.equal(temp,resolve(temp));assert.match(process.env.EXHIBITOS_NATIVE_TEST_IMAGE,/^sha256:[0-9a-f]{64}$/);
  const scope=await mkdtemp(join(temp,'exhibitos-linux-service-'));const source=join(scope,'source'),destination=join(scope,'restored');
  const {packageLocalRuntime}=await import(pathToFileURL(join(platform,'scripts/package-local-runtime.mjs')));
  const {loadViewerFixtures}=await import(pathToFileURL(join(platform,'scripts/viewer-fixtures.mjs')));
  stage='package-install';const {manifest}=await packageLocalRuntime(source,{engine:'docker',targetPlatform:'linux/amd64'});
  call(source,'install');checks.push('actual Manager install completed');
  stage='start';call(source,'start');await ready(source);checks.push('actual start readiness and protocol');
  const envBytes=await readFile(join(source,'runtime.env'));const settings=Object.fromEntries(envBytes.toString().trim().split('\n').map(l=>{const p=l.indexOf('=');assert(p>0);return[l.slice(0,p),l.slice(p+1)];}));
  assert.deepEqual(Object.keys(settings).sort(),['ADMIN_PASSWORD','ADMIN_SUBJECT','DATABASE_URL','EXHIBITOS_PORT','POSTGRES_PASSWORD','TENANT_ID']);
  const origin=manifest.openUrl,api=await client(origin,settings),prefix='/api/v1/tenants/'+settings.TENANT_ID;
  stage='private-api-import';assert.equal((await fetch(origin+prefix+'/cms/artists')).status,401);
  await api('POST',prefix+'/cms/artists',{name:'denied synthetic',bio:'synthetic'},403,{'x-csrf-token':'invalid'});
  const artist=await(await api('POST',prefix+'/cms/artists',{name:'Synthetic Linux restore artist',bio:'Public source fixture'},201)).json();
  const rights={holder:'Synthetic fixture owner',ownership:'owner',licenseId:'CC0-1.0',permissions:{display:true,download:true,export:true,commercial:false},creditLine:'Synthetic Linux qualification'};
  const artwork=await(await api('POST',prefix+'/cms/artworks',{artistId:artist.id,title:'Synthetic restored GLB',description:'Actual import and fresh restore',medium:'Synthetic GLB',creationYear:2026,dimensions:{width:1,height:1,depth:1,unit:'m'},rights,provenance:{source:'human-authored',sourceUnits:'m',scaleApplied:true,notes:'Public deterministic synthetic geometry'}},201)).json();
  const bytes=loadViewerFixtures().find(f=>f.type==='sculpture').bytes,expectedHash=digest(bytes);
  const job=await(await api('POST',prefix+'/imports',{artworkId:artwork.id,idempotencyKey:randomUUID(),mime:'model/gltf-binary',sha256:expectedHash,bytes:bytes.length,scaleMeters:1,rights},201)).json();
  await api('PUT',prefix+'/imports/'+job.id+'/bytes',bytes);await api('POST',prefix+'/imports/'+job.id+'/complete');
  let asset;
  for(let i=0;i<120;i++){const j=await(await api('GET',prefix+'/imports/'+job.id)).json();assert.notEqual(j.state,'failed');if(j.state==='approved'){asset=j.assetId;break;}await new Promise(r=>setTimeout(r,250));}assert(asset);
  const assetPath=prefix+'/assets/'+asset+'/bytes';assert.equal(digest(Buffer.from(await(await api('GET',assetPath)).arrayBuffer())),expectedHash);
  const authority=(await(await api('GET','/api/v1/freeze/authority')).json()).authority;
  checks.push('actual authenticated CMS and asynchronous GLB approval; anonymous/CSRF denial; exact blob');
  let corpus=null;if(process.env.EXHIBITOS_FROZEN_SERVICE_CHECK==='1'){stage='frozen-corpus-create';const {frozenCorpus}=await import('./qualify-linux-frozen-corpus.mjs');corpus=await frozenCorpus({platform,api,prefix,artist,rights});checks.push('deployed HTTP rich GLB/PNG/PCM scene, immutable active/revoked freezes, signed offline corpus and newer draft created');}
  stage='stop';call(source,'stop');assert.equal(call(source,'status').readiness.ready,false,'STOP_READINESS_INVALID');stage='start-again';call(source,'start');await ready(source);stage='restart';call(source,'restart');await ready(source);stage='restart-login';
  const restarted=await client(origin,settings);stage='restart-blob';assert.equal(digest(Buffer.from(await(await restarted('GET',assetPath)).arrayBuffer())),expectedHash,'RESTART_BLOB_CHANGED');stage='restart-authority';assert.deepEqual((await(await restarted('GET','/api/v1/freeze/authority')).json()).authority,authority);checks.push('actual stop/start/restart retains bytes and signing authority');
  stage='backup-key';const key=join(scope,'key.bin');await writeFile(key,randomBytes(32),{mode:0o600,flag:'wx'});
  stage='backup-create';const backup=call(source,'create-backup',process.env.EXHIBITOS_NATIVE_TEST_IMAGE,key,'--external-writers-quiesced');
  stage='backup-inventory';const archive=join(source,'backup-creation-'+backup.id,'archive'),archiveBefore=await inventory(archive),keyHash=await hashed(key);
  checks.push('actual encrypted service backup created, authenticated by producer and terminal job completed');
  stage='backup-authenticate';const verifierInput=join(scope,'verification-input');await cp(archive,verifierInput,{recursive:true,errorOnExist:true,force:false});assert.deepEqual(await inventory(verifierInput),archiveBefore);call(source,'verify-backup',process.env.EXHIBITOS_NATIVE_TEST_IMAGE,key,verifierInput);assert.deepEqual(await inventory(verifierInput),archiveBefore);checks.push('separate private input authenticates exact encrypted service archive; original source archive preserved');
  stage='fresh-restore-command';const restored=call(destination,'restore-backup',process.env.EXHIBITOS_NATIVE_TEST_IMAGE,key,archive,'13201','--fresh-installation');checks.push('actual fresh restoration receipt and persisted completed job');stage='fresh-restore-readiness';await ready(destination);
  stage='fresh-restore-login';const restoredApi=await client('http://127.0.0.1:13201',settings);stage='fresh-restore-artists';const artists=await(await restoredApi('GET',prefix+'/cms/artists')).json();assert(JSON.stringify(artists).includes(artist.id),'RESTORED_ARTIST_MISSING');stage='fresh-restore-blob';
  assert.equal(digest(Buffer.from(await(await restoredApi('GET',assetPath)).arrayBuffer())),expectedHash,'RESTORED_BLOB_CHANGED');stage='fresh-restore-authority';assert.deepEqual((await(await restoredApi('GET','/api/v1/freeze/authority')).json()).authority,{...authority,origin:'http://127.0.0.1:13201'},'RESTORED_AUTHORITY_CHANGED');
  stage='fresh-restore-anonymous';assert.equal((await fetch('http://127.0.0.1:13201'+assetPath)).status,401);checks.push('fresh root/port restore runs; credentials, metadata, blob digest and exact signing key retained; authority origin explicitly remapped; private denial retained');
  if(corpus){stage='remapped-origin-frozen-denial';await corpus.changedOrigin(restoredApi);checks.push('remapped origin refuses old frozen authorization while preserving original signed manifest');call(destination,'stop');stage='same-origin-fresh-restore';const sameOriginRoot=join(scope,'restored-same-origin');call(sameOriginRoot,'restore-backup',process.env.EXHIBITOS_NATIVE_TEST_IMAGE,key,archive,'13200','--fresh-installation');await ready(sameOriginRoot);stage='same-origin-frozen-preservation';frozenReport=await corpus.sameOrigin(await client(origin,settings));checks.push('second fresh namespace at explicitly preserved trusted origin retains rich draft/artwork/media, immutable freeze OEX/runtime bytes, revocation history, old/new signed grants and real OEX worker import');call(sameOriginRoot,'stop');}
  stage='preservation';assert.deepEqual(await inventory(archive),archiveBefore);assert.equal(await hashed(key),keyHash);assert.equal(digest(await readFile(join(source,'runtime.env'))),digest(envBytes));
  call(destination,'stop');call(source,'stop');checks.push('source credentials, external key and every archive byte/mode retained; both exact installations stopped');
  const report={format:1,state:'PASS',scope:'actual Linux Manager install/runtime/encrypted service backup/fresh namespace restore and private user-flow; not changed migration/crash/full frozen browser corpus/GUI/device/release',frozenReport,checks,sourceBundle:manifest.bundleId,backupJob:backup.id,restorationJob:restored.id,blobBytes:bytes.length,blobSha256:expectedHash,archiveFiles:archiveBefore.length,archiveBytes:archiveBefore.reduce((n,r)=>n+r.bytes,0),secretsLogged:false};
  await writeFile(join(scope,'qualification.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600,flag:'wx'});console.log(JSON.stringify(report));
} catch(error) {console.error(JSON.stringify({state:'FAIL',stage,code:/^[A-Z][A-Z0-9_]{0,79}$/.test((error.message??'').split('\n')[0])?error.message.split('\n')[0]:'LINUX_RUNTIME_QUALIFICATION_FAILED',nativeCause:/^[A-Z][A-Z0-9_]{0,79}$/.test(error.cause?.code??'')?error.cause.code:null,nativeJobFailure,lastHttpFailure,completedChecks:checks}));process.exitCode=1;}
