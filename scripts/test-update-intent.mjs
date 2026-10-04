// SPDX-License-Identifier: Apache-2.0
// Actual app-closed trust CLI, independent transient Node/OpenSSL test keys.
import assert from 'node:assert/strict';
import {generateKeyPairSync,sign,createHash} from 'node:crypto';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,chmodSync,readdirSync,renameSync,existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync,spawn} from 'node:child_process';
const cli=resolve(process.argv[2]??'target/release/exhibitos-update');
const base=mkdtempSync(join(tmpdir(),'exhibitos-update-intent-proof-'));chmodSync(base,0o700);
const profile=join(base,'profile');mkdirSync(profile,{mode:0o700});
const checks=[],hash=b=>createHash('sha256').update(b).digest('hex');
const key=generateKeyPairSync('ed25519'),next=generateKeyPairSync('ed25519');
const raw=k=>k.publicKey.export({type:'spki',format:'der'}).subarray(-32);
const now=Math.floor(Date.now()/1000);let n=0;
const put=(v,name='fixture')=>{const p=join(base,`${name}-${n++}.json`);writeFileSync(p,JSON.stringify(v),{mode:0o600});return p;};
const data=Buffer.alloc(2*1024*1024,0x73),artifact=join(base,'runtime.tar');writeFileSync(artifact,data,{mode:0o600});
const policy={format:1,channel:'development',target:'linux-arm64',protocolVersion:1,sourceSchemaSha256:'a'.repeat(64),minimumSequence:40,minimumIssuedAt:now-20,publicKeys:[raw(key).toString('hex')]};
const originalPolicy=put(policy,'trusted-policy');
const release={format:1,product:'ExhibitOS/runtime',channel:'development',target:'linux-arm64',version:'0.2.0-dev.2',sequence:41,issuedAt:now-10,expiresAt:now+3600,protocolVersion:1,sourceSchemas:['a'.repeat(64)],artifact:{name:'runtime.tar',bytes:data.length,sha256:hash(data),runtimeImageSha256:'b'.repeat(64),schemaSha256:'c'.repeat(64)}};
function seal(r=release,k=key){const payload=JSON.stringify(r),bytes=Buffer.from(payload),length=Buffer.alloc(8);length.writeBigUInt64BE(BigInt(bytes.length));return {format:1,algorithm:'ed25519',keyId:hash(raw(k)),payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),length,bytes]),k.privateKey).toString('hex')};}
const envelope=put(seal(),'signed-release');
function call(command,extra=[],error=null,p=profile){const x=spawnSync(cli,[command,'--profile',p,'--installation','default',...extra,'--apps-closed'],{encoding:'utf8',timeout:90000});assert.equal(x.error,undefined);assert.equal(x.stderr,'');const value=JSON.parse(x.stdout);if(error){assert.notEqual(x.status,0);assert.equal(value.code,error);}else {assert.equal(x.status,0,JSON.stringify(value));assert.equal(value.activated??value.trust?.activated,false);}return value;}
const status=()=>call('trust-status');
const accept=(r=envelope,f=artifact,error=null)=>call('accept',['--release',r,'--artifact',f],error);
const rotate=(p,g,error=null)=>call('trust-policy',['--policy',p,'--expected-generation',String(g)],error);
const step=s=>{checks.push(s);console.log('PASS '+s);};
call('trust-status',[],'UPDATE_TRUST_MISSING');const initial=call('trust-provision',['--policy',originalPolicy]);assert.equal(initial.generation,1);call('trust-provision',['--policy',originalPolicy],'UPDATE_TRUST_EXISTS');step('explicit separately provisioned trust; missing never bootstraps and existing never overwritten');
const bad=seal();bad.signature='00'.repeat(64);accept(put(bad),artifact,'UPDATE_SIGNATURE_INVALID');
const changedDir=join(base,'changed');mkdirSync(changedDir,{mode:0o700});const changed=join(changedDir,'runtime.tar');writeFileSync(changed,Buffer.alloc(data.length,0),{mode:0o600});accept(envelope,changed,'UPDATE_ARTIFACT_MISMATCH');assert.equal(status().generation,1);step('real failed signatures and same-length changed artifacts never advance persisted floors');
const accepted=accept();assert.equal(accepted.verification.artifactVerified,true);assert.equal(accepted.verification.backupRestoreVerified,false);assert.equal(accepted.trust.minimumSequence,41);assert.equal(status().generation,2);accept(envelope,artifact,'UPDATE_RELEASE_REPLAY');step('actual 2MiB artifact acceptance survives separate CLI processes and refuses replay');
renameSync(profile,join(base,'retained-original-profile'));accept(envelope,artifact,'UPDATE_RELEASE_REPLAY');mkdirSync(profile,{mode:0o700});accept(envelope,artifact,'UPDATE_RELEASE_REPLAY');assert.equal(readdirSync(profile).length,0);step('real profile removal/replacement retains external trust and does not write replacement profile');
const r42=put(seal({...release,sequence:42,issuedAt:now-5}));accept(r42);const stored=status();assert.equal(stored.minimumSequence,42);assert.equal(stored.minimumIssuedAt,now-5);
const rotated={...policy,minimumSequence:42,minimumIssuedAt:now-5,publicKeys:[raw(next).toString('hex')]};const replacement=put(rotated,'authorized-rotation');rotate(replacement,2,'UPDATE_POLICY_STALE');assert.equal(status().policyGeneration,1);const rotatedReceipt=rotate(replacement,1);assert.equal(rotatedReceipt.policyGeneration,2);accept(put(seal({...release,sequence:43,issuedAt:now-4})),artifact,'UPDATE_KEY_UNTRUSTED');step('generation-bound administrator rotation persists revoked original signing key across processes');
rotate(put({...rotated,publicKeys:policy.publicKeys}),2,'UPDATE_TRUST_INVALID');rotate(put({...rotated,minimumSequence:41}),2,'UPDATE_TRUST_ROLLBACK');rotate(put({...rotated,channel:'stable'}),2,'UPDATE_TRUST_ROLLBACK');assert.equal(status().generation,4);step('revoked key reintroduction and policy floor/channel regression refuse without new records');
const current=accept(put(seal({...release,sequence:43,issuedAt:now-4},next)));assert.equal(current.trust.minimumSequence,43);assert.equal(current.trust.policyGeneration,2);step('authorized new signing key verifies actual bytes and durably consumes next generation');
const store=join(base,readdirSync(base).find(n=>n.startsWith('.exhibitos-release-trust-'))),marker=join(base,'lock-ready');
const holder=spawn('python3',['-c','import fcntl,sys,time; f=open(sys.argv[1],"r"); fcntl.flock(f,fcntl.LOCK_EX); open(sys.argv[2],"w").write("ready"); time.sleep(20)',join(store,'trust.lock'),marker],{stdio:'ignore'});
try {for(let i=0;i<200&&!existsSync(marker);i++)await new Promise(r=>setTimeout(r,25));assert.equal(existsSync(marker),true);process.kill(holder.pid,0);call('trust-status',[],'UPDATE_TRUST_BUSY');} finally {holder.kill('SIGTERM');await new Promise(r=>holder.once('exit',r));}
assert.equal(status().minimumSequence,43);step('actual independent Python-held filesystem lock fences concurrent trust CLI');
assert.equal(hash(readFileSync(artifact)),hash(data));assert.equal(readFileSync(originalPolicy,'utf8'),JSON.stringify(policy));assert.equal(readFileSync(envelope,'utf8'),JSON.stringify(seal()));assert.equal(existsSync(join(base,'retained-original-profile')),true);step('original artifact/policy/envelope/profile and all journal generations retained; private test keys never exported');

const plan={operationId:'intent-1',sourceInstance:'source-1',targetInstance:'target-1',sourceImage:'d'.repeat(64),targetImage:'b'.repeat(64),sourceSchema:'a'.repeat(64),targetSchema:'c'.repeat(64),backupId:'backup-1',backupManifest:'e'.repeat(64),sourceInventory:'f'.repeat(64),requiredFreeBytes:1024};
const planPath=put(plan,'private-plan');
function freshProfile(name){const p=join(base,name);mkdirSync(p,{mode:0o700});call('trust-provision',['--policy',originalPolicy],null,p);return p;}
function prepare(p,planFile=planPath,r=envelope,error=null){return call('prepare-update',['--release',r,'--artifact',artifact,'--plan',planFile],error,p);}
const intentProfile=freshProfile('new-intent-profile');const prepared=prepare(intentProfile);assert.equal(prepared.trust.generation,2);assert.equal(prepared.trust.minimumSequence,41);assert.equal(prepared.executed,false);assert.equal(prepared.intent.update.stage,'prepared');assert.equal(prepared.intent.update.preflight,null);assert.deepEqual(prepared.intent.update.plan,plan);
const resumed=call('update-intent',[],null,intentProfile);assert.deepEqual(resumed.intent,prepared.intent);prepare(intentProfile,planPath,envelope,'UPDATE_OPERATION_PENDING');assert.equal(readdirSync(intentProfile).length,0);step('actual signed bytes and bound update intent commit in one journal generation, survive process restart and prevent conflicting preparation');
const consumedProfile=freshProfile('already-accepted-profile');call('accept',['--release',envelope,'--artifact',artifact],null,consumedProfile);const recoveredIntent=prepare(consumedProfile);assert.equal(recoveredIntent.trust.generation,3);assert.equal(recoveredIntent.trust.minimumSequence,41);assert.equal(recoveredIntent.intent.update.stage,'prepared');step('exact previously accepted envelope prepares with unchanged security floor, actual artifact rehash and no reacceptance');
const mismatchProfile=freshProfile('mismatched-plan-profile');prepare(mismatchProfile,put({...plan,targetImage:'0'.repeat(64)}),envelope,'UPDATE_PLAN_MISMATCH');assert.equal(call('trust-status',[],null,mismatchProfile).generation,1);assert.equal(call('update-intent',[],null,mismatchProfile).intent,null);step('actual wrong-image intent fails without consuming release or writing a prepared operation');
renameSync(intentProfile,join(base,'retained-intent-profile'));const afterRemoval=call('update-intent',[],null,intentProfile);assert.deepEqual(afterRemoval.intent,prepared.intent);assert.equal(afterRemoval.trust.minimumSequence,41);step('actual profile namespace removal preserves original intent and floor without recreating profile or executing runtime');

// Synthetic trusted journal event: exercises actual separate CLI-process recovery,
// not a real engine, backup-restoration or preflight verifier.
const recoveryStore=join(base,readdirSync(base).filter(n=>n.startsWith('.exhibitos-release-trust-')).find(n=>{
    const last=join(base,n,'00000000000000000003.json');
    if(!existsSync(last))return false;
    const r=JSON.parse(readFileSync(last));return r.intent?.update?.plan?.operationId==='intent-1';
}));
const previousPath=join(recoveryStore,'00000000000000000003.json');
const previousBytes=readFileSync(previousPath),applying=JSON.parse(previousBytes);
const preflight={plan,signatureVerified:true,artifactVerified:true,compatibilityVerified:true,backupRestoreVerified:true,currentSourceMatchesBackup:true,availableFreeBytes:1024,imageOnlyRollbackVerified:false};
applying.generation=4;applying.previousSha256=hash(previousBytes);
applying.intent.update.stage='applying';applying.intent.update.preflight=preflight;
applying.updateEvent={kind:'begin',evidence:preflight};
const applyingPath=join(recoveryStore,'00000000000000000004.json');
writeFileSync(applyingPath,JSON.stringify(applying),{mode:0o600});
const applyingHash=hash(readFileSync(applyingPath));
const recovered=call('update-intent',[],null,consumedProfile);
assert.equal(recovered.trust.generation,5);assert.equal(recovered.trust.minimumSequence,41);
assert.equal(recovered.intent.update.stage,'recovery_required');assert.equal(recovered.intent.update.failure,'interrupted');
assert.deepEqual(recovered.intent.update.plan,plan);
assert.equal(call('update-intent',[],null,consumedProfile).trust.generation,5);
assert.equal(hash(readFileSync(applyingPath)),applyingHash);assert.deepEqual(readFileSync(previousPath),previousBytes);
step('actual CLI open durably marks synthetic Applying as Interrupted/RecoveryRequired exactly once, retaining plan, floors and original records; no engine proof');

const report={format:1,checks,recovered,prepared,recoveredIntent,fixture:base,cliSha256:hash(readFileSync(cli)),initial,accepted,current,final:status(),journal:readdirSync(store),limits:['synthetic artifact, not a deployable OCI bundle; no engine update/migration/activation/restore','trusted local OS administrator policy replacement, no signed remote rotation/production root','same-UID or complete security-journal rollback not resisted; retain external journal during profile recovery','no native GUI/Windows/Linux filesystem qualification; private signing keys transient memory only']};
const path=join(base,'update-intent-report.json');writeFileSync(path,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log('Report '+path);console.log('SHA256 '+hash(readFileSync(path)));
