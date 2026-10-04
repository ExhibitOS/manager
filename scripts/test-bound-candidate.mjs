// SPDX-License-Identifier: Apache-2.0
// Real registered target restoration from a retained synthetic Manager backup.
// No original path moves, data deletion, release activation, or exported signing key.
import assert from 'node:assert/strict';
import {generateKeyPairSync, sign, createHash, randomUUID} from 'node:crypto';
import {mkdtempSync, mkdirSync, chmodSync, readFileSync, writeFileSync, copyFileSync, readdirSync, realpathSync, existsSync, statSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg, managerArg, fixtureArg, ...resumeArgs] = process.argv.slice(2);
const genuine = resumeArgs[0] === '--genuine-release';
if (genuine) assert.ok(resumeArgs.length===4 && resumeArgs[2]==='--workspace-parent');
const resuming = !genuine && resumeArgs.length > 0;
if (resuming) assert.ok(resumeArgs.length === 6 && resumeArgs[0] === '--resume-existing' && resumeArgs[2] === '--archive-baseline' && resumeArgs[4] === '--producer-record');
assert.ok(fixtureArg, 'usage: node scripts/test-bound-candidate.mjs <update CLI> <manager CLI> <retained synthetic creation fixture>');
const cli = resolve(cliArg), manager = resolve(managerArg), fixture = realpathSync(fixtureArg);
const creation = JSON.parse(readFileSync(join(fixture, 'backup-creation-report.json')));
const original = join(fixture, 'retained-source-manager');
const archive = join(original, 'backup-creation-' + creation.receipt.id, 'archive');
const keyFile = join(fixture, 'key.bin'), image = creation.receipt.image;
const plain = readFileSync(join(fixture, 'restore-work/restored/manifest.json'));
const backup = JSON.parse(plain), hash = b => createHash('sha256').update(b).digest('hex');
assert.equal(hash(plain), creation.receipt.authenticatedManifestSha256);
assert.equal(backup.id, creation.receipt.backupId);
function canonical(value) {
    if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
    if (value && typeof value === 'object') return '{' + Object.keys(value).sort((a,b) => Buffer.compare(Buffer.from(a), Buffer.from(b))).map(k => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
    if (typeof value === 'number') assert.ok(Number.isSafeInteger(value));
    return JSON.stringify(value);
}
const inventoryHash = hash(canonical(backup.inventory));
const oldManifest = JSON.parse(readFileSync(join(original, 'bundle/manifest.json')));
const imageInventory = JSON.parse(readFileSync(join(fixture, 'restore-work/restored/configuration/manager-image-inventory.json')));
const platform = imageInventory.find(i => i.reference === oldManifest.images[0].reference);
assert.ok(platform && platform.contentId.startsWith('sha256:'));
const sourceImage = platform.contentId.slice(7);
const sourceSchema = hash(canonical({schemaDigest:backup.inventory.schemaDigest,schemaVersion:backup.inventory.schemaVersion,migrations:backup.inventory.migrations}));
const workspaceParent = genuine ? realpathSync(resumeArgs[3]) : realpathSync(tmpdir());
if(genuine) { const m=statSync(workspaceParent);assert.equal(m.uid,process.getuid());assert.equal(m.mode&0o777,0o700); }
const base = resuming ? realpathSync(resumeArgs[1]) : realpathSync(mkdtempSync(join(workspaceParent, 'exhibitos-bound-candidate-proof-')));
assert.ok(base.startsWith(workspaceParent + '/exhibitos-bound-candidate-proof-')); chmodSync(base, 0o700);
const producer = resuming ? JSON.parse(readFileSync(resumeArgs[5])) : null;
if (producer) assert.equal(producer.fixture,base);
const files = ['installed.json','engine.json','runtime.env','bundle/manifest.json','bundle/compose.yaml'];
const sourceHashes = () => Object.fromEntries(files.map(n => [n, hash(readFileSync(join(original,n)))]));
function inventory(path) {
    return Object.fromEntries(readdirSync(path,{withFileTypes:true}).flatMap(e => e.isDirectory() ? Object.entries(inventory(join(path,e.name))).map(([n,h]) => [e.name+'/'+n,h]) : [[e.name,hash(readFileSync(join(path,e.name)))]]));
}
function processResult(executable,args) {
    const result = spawnSync(executable,args,{encoding:'utf8',timeout:900000});
    assert.equal(result.error,undefined); assert.equal(result.stderr,'');
    return {status:result.status,value:JSON.parse(result.stdout)};
}
function docker(...args) {
    const result = spawnSync('docker',args,{encoding:null,timeout:120000});
    assert.equal(result.error,undefined); assert.equal(result.status,0,'Synthetic Docker command failed'); return result.stdout;
}
function sourceStates() { return docker('ps','-a','--filter','label=com.docker.compose.project='+oldManifest.projectName,'--format','{{.ID}} {{.State}}').toString().split('\n').filter(Boolean).sort(); }
// On resume, copied source deployment files predate the original restore, and
// an independently retained encrypted archive supplies the pre-run byte baseline.
const oldFiles = resuming ? Object.fromEntries(files.map(n => [n,hash(readFileSync(join(base,'profile-1/local-runtime',n)))])) : sourceHashes();
const oldArchive = resuming ? inventory(realpathSync(resumeArgs[3])) : inventory(archive);
if (resuming) assert.equal(JSON.parse(readFileSync(join(resumeArgs[3],'complete.json'))).id,backup.id);
const oldKeyHash = hash(readFileSync(keyFile)), oldStates = sourceStates();
assert.deepEqual(sourceHashes(),oldFiles); assert.deepEqual(inventory(archive),oldArchive);
const checkpoint = join(base,'source-baseline.json');
if (!resuming) writeFileSync(checkpoint,JSON.stringify({files:oldFiles,archive:oldArchive,keySha256:oldKeyHash,sourceStates:oldStates}),{mode:0o600});
assert.ok(oldStates.length && oldStates.every(s => s.endsWith('exited')), 'Retained synthetic source must already be stopped');
const checks = [], step = text => { checks.push(text); console.log('PASS '+text); };
const kp = generateKeyPairSync('ed25519'), raw = kp.publicKey.export({type:'spki',format:'der'}).subarray(-32), now = Math.floor(Date.now()/1000);
const genuineRoot = genuine ? realpathSync(resumeArgs[1]) : null;
const genuineProof = genuine ? JSON.parse(readFileSync(join(genuineRoot,'oci-final-proof.json'))) : null;
const artifact = join(base,'runtime.tar'), data = genuine ? readFileSync(join(genuineRoot,'runtime.tar')) : Buffer.alloc(1024,0x73);
if(genuine) { assert.equal(data.length,genuineProof.artifactBytes); assert.equal(hash(data),genuineProof.artifactSha256); assert.equal(sourceSchema,genuineProof.sourceSchemaSha256); assert.equal(genuineProof.target,'linux-arm64'); }
if (!resuming) writeFileSync(artifact,data,{mode:0o600}); else assert.equal(hash(readFileSync(artifact)),hash(data));
function put(value,name) { const path=join(base,name+'.json');writeFileSync(path,JSON.stringify(value),{mode:0o600});return path; }
const policyFile = resuming ? join(base,'policy.json') : put({format:1,channel:'development',target:'linux-arm64',protocolVersion:1,sourceSchemaSha256:sourceSchema,minimumSequence:40,minimumIssuedAt:now-20,publicKeys:[raw.toString('hex')]},'policy');
const release = {format:1,product:'ExhibitOS/runtime',channel:'development',target:'linux-arm64',version:genuine ? genuineProof.version : '0.2.0-dev.2',sequence:41,issuedAt:now-10,expiresAt:now+3600,protocolVersion:1,sourceSchemas:[sourceSchema],artifact:{name:'runtime.tar',bytes:data.length,sha256:hash(data),runtimeImageSha256:genuine ? genuineProof.runtimeImageSha256 : 'b'.repeat(64),schemaSha256:genuine ? genuineProof.targetSchemaSha256 : 'c'.repeat(64)}};
const payload=JSON.stringify(release), bytes=Buffer.from(payload), length=Buffer.alloc(8);length.writeBigUInt64BE(BigInt(bytes.length));
const releaseFile=resuming ? join(base,'release.json') : put({format:1,algorithm:'ed25519',keyId:hash(raw),payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),length,bytes]),kp.privateKey).toString('hex')},'release');
function call(profile,command,extra=[],error=null) {
    const r=processResult(cli,[command,'--profile',profile,'--installation','default',...extra,'--apps-closed']);
    if(error) { assert.notEqual(r.status,0);assert.equal(r.value.code,error); }
    else assert.equal(r.status,0,JSON.stringify(r.value)); return r.value;
}
let n=0;
function setup(manifestHash=hash(plain)) {
    const profile=join(base,'profile-'+n++), sourceId=randomUUID(), targetId=randomUUID();
    mkdirSync(profile,{mode:0o700}); const root=join(profile,'local-runtime');mkdirSync(root,{mode:0o700});mkdirSync(join(root,'bundle'),{mode:0o700});
    for(const file of files) { copyFileSync(join(original,file),join(root,file));chmodSync(join(root,file),0o600); }
    mkdirSync(join(profile,'installations'),{mode:0o700});const target=join(profile,'installations',targetId);mkdirSync(target,{mode:0o700});
    const registry=JSON.stringify({format:1,activeId:sourceId,installations:[{id:sourceId,kind:'default',createdAt:0},{id:targetId,kind:'recovery',createdAt:0}]});
    writeFileSync(join(profile,'installation-selection.json'),registry,{mode:0o600});
    call(profile,'trust-provision',['--policy',policyFile]);
    const plan={operationId:randomUUID(),sourceInstance:sourceId,targetInstance:targetId,sourceImage,targetImage:release.artifact.runtimeImageSha256,sourceSchema,targetSchema:release.artifact.schemaSha256,backupId:backup.id,backupManifest:manifestHash,sourceInventory:inventoryHash,requiredFreeBytes:2*1024*1024*1024};
    call(profile,'prepare-update',['--release',releaseFile,'--artifact',artifact,'--plan',put(plan,'plan-'+n)]);
    return {profile,target,registry,plan};
}
// Reserve a fresh host loopback port using Python; the restore adapter reserves it again.
const portProcess=spawnSync('python3',['-c','import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'],{encoding:'utf8'});assert.equal(portProcess.status,0);const port=portProcess.stdout.trim();
const extra=['--maintenance-image',image,'--key',keyFile,'--archive',archive,'--port',port,'--fresh-candidate','--external-writers-quiesced'];
function existing(index) {
    const profile=join(base,'profile-'+index), plan=JSON.parse(readFileSync(join(base,'plan-'+(index+1)+'.json')));
    const registry=readFileSync(join(profile,'installation-selection.json'),'utf8');
    assert.equal(JSON.parse(registry).activeId,plan.sourceInstance);
    assert.equal(plan.sourceSchema,sourceSchema);assert.equal(plan.sourceInventory,inventoryHash);assert.equal(plan.backupId,backup.id);
    return {profile,target:join(profile,'installations',plan.targetInstance),registry,plan};
}
const wrong=resuming?existing(0):setup('0'.repeat(64));
if (!resuming) call(wrong.profile,'prepare-update-candidate',extra,'RESTORE_FAILED');
const failure=JSON.parse(readFileSync(join(wrong.target,'restoration.json')));assert.equal(failure.errorCode,'UPDATE_RESTORE_BINDING_MISMATCH');assert.equal(existsSync(join(wrong.target,'installed.json')),false);
assert.equal(readFileSync(join(wrong.profile,'installation-selection.json'),'utf8'),wrong.registry);
assert.equal(call(wrong.profile,'update-intent').intent.update.stage,'prepared');
step('real authenticated foreign manifest refuses before image import/target installation and preserves active selection/prepared intent');
const selected=resuming?existing(1):setup();
let prepared;
if (resuming) {
    const job=JSON.parse(readFileSync(join(selected.target,'restoration.json')));assert.equal(job.state,'completed');
    const receipt=JSON.parse(readFileSync(join(selected.target,'restore-'+job.id,'receipt.json')));
    const intent=call(selected.profile,'update-intent');assert.equal(intent.trust.generation,2);
    // The previous actual CLI completed restoration; resume only observation and
    // repeat refusal, not an engine restore or reconstructed update preflight.
    prepared={restoration:receipt,intent:intent.intent,updateExecuted:false,preflightVerified:false,candidatePrepared:true};
} else prepared=call(selected.profile,'prepare-update-candidate',extra);
assert.equal(prepared.updateExecuted,false);assert.equal(prepared.preflightVerified,false);assert.equal(prepared.candidatePrepared,true);
assert.equal(prepared.restoration.backupId,backup.id);assert.equal(prepared.restoration.authenticatedManifestSha256,hash(plain));
assert.deepEqual(prepared.restoration.sourceVerification,{inventorySha256:inventoryHash,schemaSha256:sourceSchema,runtimeImageSha256:sourceImage});
assert.equal(prepared.intent.update.stage,'prepared');assert.equal(prepared.intent.update.preflight,null);
const recovered=JSON.parse(readFileSync(join(selected.target,'bundle/manifest.json')));
assert.notEqual(recovered.projectName,oldManifest.projectName);assert.notEqual(recovered.bundleId,oldManifest.bundleId);
assert.equal(readFileSync(join(selected.profile,'installation-selection.json'),'utf8'),selected.registry);
step('actual original DB/blob/config/images restore into exact registered fresh target, bound backup/inventory/schema/image proof and independent Runtime readiness');
function managerCall(action) { const r=processResult(manager,['--root',selected.target,action]);assert.equal(r.status,0,JSON.stringify(r.value));return r.value; }
if (resuming && managerCall('status').state==='stopped') managerCall('start');
assert.equal(managerCall('status').state,'running');assert.equal(managerCall('restoration-status').state,'completed');
const compose=['compose','--env-file',join(selected.target,'runtime.env'),'--project-name',recovered.projectName,'--file',join(selected.target,'bundle/compose.yaml')];
const db=docker(...compose,'ps','-a','--quiet','database').toString().trim(),app=docker(...compose,'ps','-a','--quiet','platform').toString().trim();
assert.equal(docker('inspect',app,'--format','{{.Image}}').toString().trim(),'sha256:'+sourceImage);
assert.equal(docker('exec',db,'psql','-U','exhibitos','-d','exhibitos','-Atc','SELECT witness FROM synthetic_manager_backup WHERE id=1').toString().trim(),'Manager backup original data');
assert.equal(docker('exec',app,'node','--input-type=module','-e',"import{readFile}from'node:fs/promises';process.stdout.write(await readFile('/data/blobs/synthetic/manager-proof'))").toString(),'synthetic Manager blob');
const expectedSigning=readFileSync(join(fixture,'restore-work/restored/configuration/freeze-signing-key.json'));
assert.equal(hash(docker('exec',app,'node','--input-type=module','-e',"import{readFile}from'node:fs/promises';process.stdout.write(await readFile('/data/config/freeze-signing-key.json'))")),hash(expectedSigning));
step('actual PostgreSQL witness, synthetic blob and preserved freeze signing key verified in running candidate');
const env=Object.fromEntries(readFileSync(join(selected.target,'runtime.env'),'utf8').trim().split('\n').map(line=>{const at=line.indexOf('=');return[line.slice(0,at),line.slice(at+1)];}));
const url=prepared.restoration.openUrl;
const login=await fetch(url+'/api/v1/auth/login',{method:'POST',headers:{'Content-Type':'application/json','Origin':url},body:JSON.stringify({subject:env.ADMIN_SUBJECT,password:env.ADMIN_PASSWORD,tenantId:env.TENANT_ID})});assert.equal(login.status,200);assert.ok(login.headers.get('set-cookie'));
const web=await fetch(url);assert.equal(web.status,200);assert.ok((await web.text()).length>100);
step('preserved administrator credentials and actual candidate HTTP login/web work');
const installedHash=hash(readFileSync(join(selected.target,'installed.json'))),receiptHash=hash(readFileSync(join(selected.target,'restore-'+prepared.restoration.id,'receipt.json')));
call(selected.profile,'prepare-update-candidate',extra,'RESTORE_FRESH_ROOT_REQUIRED');
assert.equal(hash(readFileSync(join(selected.target,'installed.json'))),installedHash);assert.equal(hash(readFileSync(join(selected.target,'restore-'+prepared.restoration.id,'receipt.json'))),receiptHash);
step('second attempt refuses existing target without overwriting completed candidate or receipt');
managerCall('stop');assert.equal(managerCall('status').state,'stopped');
const intent=call(selected.profile,'update-intent');assert.equal(intent.trust.generation,2);assert.equal(intent.intent.update.stage,'prepared');assert.equal(intent.intent.update.preflight,null);
assert.deepEqual(sourceHashes(),oldFiles);assert.deepEqual(inventory(archive),oldArchive);assert.equal(hash(readFileSync(keyFile)),oldKeyHash);assert.deepEqual(sourceStates(),oldStates);
step('new candidate stopped with volumes/data retained; original deployment/archive/key/source container states, selection and journal preserved');
const report={format:1,checks,fixture:base,resumedCompletedCandidate:resuming,producer,originalFileBaseline:resuming?'retained registered source copy before restoration':'initial persisted checkpoint',originalArchiveBaseline:resuming?realpathSync(resumeArgs[3]):archive,keysAndStatesBaseline:resuming?'resume observation; initial helper used read-only source/key mounts':'initial persisted checkpoint',cliSha256:hash(readFileSync(cli)),managerSha256:hash(readFileSync(manager)),restoration:prepared.restoration,plan:selected.plan,intentGeneration:intent.trust.generation,limits:['actual macOS Docker source-version candidate, not signed target update/migration/rollback activation','source inventory bound to archived snapshot, not current-source quiescence proof; compatibility and full preflight remain','same engine cached images imported; no cold engine/full frozen corpus/Windows/Podman/native GUI qualification','all candidates, volumes, plaintext and original data retained; signing keys not exported to Git/logs']};
const reportPath=join(base,'bound-candidate-report.json');writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:reportPath,checks:checks.length}));

if(genuine) { const identity={format:1,artifactBytes:data.length,artifactSha256:hash(data),targetImage:release.artifact.runtimeImageSha256,targetSchema:release.artifact.schemaSha256,sourceSchema,version:release.version,plan:selected.plan,fixture:base,limits:['genuine development artifact and candidate restoration only','candidate initially uses original backup Runtime, target image not applied or health-tested','fresh fixture trust policy, not production authority/current coherent security-state recovery']};writeFileSync(join(base,'genuine-release-binding.json'),JSON.stringify(identity,null,2)+'\n',{mode:0o600,flag:'wx'});console.log('GenuineBinding '+join(base,'genuine-release-binding.json'));}
