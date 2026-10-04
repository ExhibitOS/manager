// SPDX-License-Identifier: Apache-2.0
// Explicit local development fixture qualification; not a production publisher.
import assert from 'node:assert/strict';
import {generateKeyPairSync, sign, createHash, randomUUID} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync, realpathSync, readdirSync, lstatSync, openSync, readSync, closeSync} from 'node:fs';
import {join, resolve, dirname} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg,profileArg,artifactArg,parentArg,ack]=process.argv.slice(2);
assert.equal(ack,'--local-development-fixture');
assert(cliArg&&profileArg&&artifactArg&&parentArg,'usage: node scripts/test-development-renewal.mjs <CLI> <private profile> <actual artifact> <private output parent> --local-development-fixture');
const cli=realpathSync(cliArg),profile=realpathSync(profileArg),artifact=realpathSync(artifactArg),parent=realpathSync(parentArg);
assert.equal(profile,resolve(profileArg));assert.equal(artifact,resolve(artifactArg));assert(!lstatSync(artifact).isSymbolicLink());
const digest=v=>createHash('sha256').update(v).digest('hex');
function fileHash(path){const h=createHash('sha256'),fd=openSync(path,'r'),b=Buffer.alloc(65536);try{let n;while((n=readSync(fd,b,0,b.length,null)))h.update(b.subarray(0,n));}finally{closeSync(fd);}return h.digest('hex');}
function call(command,extra=[],error=null){const r=spawnSync(cli,[command,'--profile',profile,'--installation','default',...extra,'--apps-closed'],{encoding:'utf8',timeout:300000});assert.equal(r.error,undefined);assert.equal(r.stderr,'');const v=JSON.parse(r.stdout);if(error){assert.notEqual(r.status,0);assert.equal(v.code,error);}else assert.equal(r.status,0,JSON.stringify(v));return v;}
const before=call('update-intent');assert.equal(before.intent.update.stage,'prepared');assert.equal(before.intent.update.preflight,null);
const oldEnvelope=JSON.parse(before.intent.envelope),oldRelease=JSON.parse(oldEnvelope.payload),now=Math.floor(Date.now()/1000);
assert.equal(oldRelease.channel,'development');assert(now>oldRelease.expiresAt,'fixture release must actually be expired');
assert.equal(fileHash(artifact),oldRelease.artifact.sha256);assert.equal(lstatSync(artifact).size,oldRelease.artifact.bytes);
const registryPath=join(profile,'installation-selection.json'),oldRegistry=JSON.parse(readFileSync(registryPath));
const scope=digest(Buffer.from(`ExhibitOS-release-trust-v1\0${profile}\0default`)),trustRoot=join(dirname(profile),'.exhibitos-release-trust-'+scope);
const records=readdirSync(trustRoot).filter(n=>/^\d{20}\.json$/.test(n)).sort();assert.equal(records.length,before.trust.generation);
const oldRecords=Object.fromEntries(records.map(n=>[n,fileHash(join(trustRoot,n))]));
const oldRecord=JSON.parse(readFileSync(join(trustRoot,records.at(-1))));assert.equal(oldRecord.policy.channel,'development');
assert.equal(oldRecord.policy.minimumSequence,before.trust.minimumSequence);assert.equal(oldRecord.policy.minimumIssuedAt,before.trust.minimumIssuedAt);
const out=join(parent,'development-renewal-'+randomUUID());mkdirSync(out,{mode:0o700});
const checks=[];function put(name,value){const p=join(out,name);writeFileSync(p,JSON.stringify(value),{mode:0o600,flag:'wx'});return p;}
function step(name){checks.push(name);put('step-'+checks.length+'.json',{name,observed:call('update-intent')});console.log('PASS '+name);}
const oldReleasePath=put('expired-envelope.json',oldEnvelope),oldPlanPath=put('old-plan.json',before.intent.update.plan);
call('prepare-update',['--release',oldReleasePath,'--artifact',artifact,'--plan',oldPlanPath],'UPDATE_RELEASE_EXPIRED');assert.deepEqual(call('update-intent'),before);
step('actual expired old envelope refuses without changing Prepared or floors');
const kp=generateKeyPairSync('ed25519'),raw=kp.publicKey.export({type:'spki',format:'der'}).subarray(-32),id=digest(raw);
const policy={...oldRecord.policy,publicKeys:[raw.toString('hex')]};
const rollbackPolicy=put('rollback-policy.json',{...policy,minimumSequence:before.trust.minimumSequence-1});
call('trust-policy',['--policy',rollbackPolicy,'--expected-generation',String(before.trust.policyGeneration)],'UPDATE_TRUST_ROLLBACK');assert.deepEqual(call('update-intent'),before);
step('administrator floor-lowering policy refuses before writes');
const target=call('register-update-target',['--preserve-active']);assert.equal(target.activated,false);assert.equal(target.runtimeStarted,false);assert.equal(target.activeInstance,oldRegistry.activeId);assert.equal(target.sourceInstance,before.intent.update.plan.sourceInstance);
assert.notEqual(target.targetInstance,before.intent.update.plan.targetInstance);assert.equal(readdirSync(target.targetPath).length,0);
const registered=JSON.parse(readFileSync(registryPath));assert.equal(registered.activeId,oldRegistry.activeId);assert.deepEqual(registered.installations.slice(0,-1),oldRegistry.installations);assert.equal(registered.installations.at(-1).id,target.targetInstance);
assert.deepEqual(call('update-intent'),before);put('registered-target.json',target);step('fresh private target registered without activation, original entries and intent preserved');
const policyPath=put('replacement-policy.json',policy);const rotated=call('trust-policy',['--policy',policyPath,'--expected-generation',String(before.trust.policyGeneration)]);
assert.equal(rotated.minimumSequence,before.trust.minimumSequence);assert.equal(rotated.minimumIssuedAt,before.trust.minimumIssuedAt);assert.equal(rotated.policyGeneration,before.trust.policyGeneration+1);assert.deepEqual(call('update-intent').intent,before.intent);
step('development key rotation preserves floors and existing Prepared binding');
const discarded=call('discard-update-intent',['--operation-id',before.intent.update.plan.operationId,'--expected-generation',String(rotated.generation),'--preserve-data']);assert.equal(discarded.intent,null);assert.equal(discarded.trust.minimumSequence,before.trust.minimumSequence);
step('explicit Prepared discard appends history and retains consumed identities');
const release={...oldRelease,sequence:before.trust.minimumSequence+1,issuedAt:now,expiresAt:now+3600};
const payload=JSON.stringify(release),bytes=Buffer.from(payload),length=Buffer.alloc(8);length.writeBigUInt64BE(BigInt(bytes.length));
const envelope={format:1,algorithm:'ed25519',keyId:id,payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),length,bytes]),kp.privateKey).toString('hex')};
const releasePath=put('release.json',envelope),plan={...before.intent.update.plan,operationId:randomUUID(),targetInstance:target.targetInstance},planPath=put('plan.json',plan);
const reused=put('reused-operation-plan.json',{...plan,operationId:before.intent.update.plan.operationId});
call('prepare-update',['--release',releasePath,'--artifact',artifact,'--plan',reused],'UPDATE_IDENTITY_REUSED');assert.equal(call('update-intent').intent,null);
const reusedTarget=put('reused-target-plan.json',{...plan,targetInstance:before.intent.update.plan.targetInstance});
call('prepare-update',['--release',releasePath,'--artifact',artifact,'--plan',reusedTarget],'UPDATE_IDENTITY_REUSED');assert.equal(call('update-intent').intent,null);
step('old consumed operation and target refuse across rotation and discard');
const prepared=call('prepare-update',['--release',releasePath,'--artifact',artifact,'--plan',planPath]);assert.equal(prepared.executed,false);assert.equal(prepared.trust.activated,false);assert.equal(prepared.intent.update.stage,'prepared');assert.equal(prepared.intent.update.preflight,null);assert.deepEqual(prepared.intent.update.plan,plan);assert.equal(prepared.trust.minimumSequence,release.sequence);assert.equal(prepared.trust.minimumIssuedAt,now);
step('fresh real artifact/time-valid signed Prepared commits higher floors and fresh identities');
for(const [n,h] of Object.entries(oldRecords))assert.equal(fileHash(join(trustRoot,n)),h);
const finalRecords=readdirSync(trustRoot).filter(n=>/^\d{20}\.json$/.test(n)).sort();const finalRecord=JSON.parse(readFileSync(join(trustRoot,finalRecords.at(-1))));
for(const key of oldRecord.policy.publicKeys)assert(finalRecord.revokedKeys.includes(digest(Buffer.from(key,'hex'))));
assert.equal(fileHash(artifact),oldRelease.artifact.sha256);assert.equal(JSON.parse(readFileSync(registryPath)).activeId,oldRegistry.activeId);assert.equal(readdirSync(target.targetPath).length,0);
put('report.json',{completedAt:new Date().toISOString(),cliSha256:fileHash(cli),checks,oldTrust:before.trust,newTrust:prepared.trust,registeredTarget:target,oldPlan:before.intent.update.plan,newPlan:plan,artifact:{path:artifact,sha256:oldRelease.artifact.sha256,bytes:oldRelease.artifact.bytes},releasePath,planPath,policyPath,originalCommittedRecordsPreserved:true,removedKeysPermanentlyRevoked:true,privateSigningKeyPersisted:false,candidateRestored:false,preflightVerified:false,updateExecuted:false,limitations:['local ephemeral development key, not production signing','new target registered but restoration not run','earlier target-bound observations are historical and must be requalified for fresh plan','renewal is multi-command; private step receipts retained for explicit recovery, never automatic apply']});
console.log(JSON.stringify({status:'PASS',checks:checks.length,report:join(out,'report.json')}));
