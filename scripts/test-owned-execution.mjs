// SPDX-License-Identifier: Apache-2.0
// Actual local CLI + Docker authentication; only operator-supplied synthetic fixtures.
import assert from 'node:assert/strict';
import {generateKeyPairSync,sign,createHash,randomUUID,randomBytes} from 'node:crypto';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,chmodSync,readdirSync,copyFileSync,realpathSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync,spawn} from 'node:child_process';
const [cliArg,sourceArg,image,keyArg,archiveArg,expected]=process.argv.slice(2);
assert.ok(expected,'usage: node scripts/test-owned-execution.mjs <CLI> <synthetic installed root> <immutable maintenance image> <private key> <synthetic archive> <plaintext manifest sha256>');
const cli=resolve(cliArg),source=resolve(sourceArg),keyFile=resolve(keyArg),archive=resolve(archiveArg);
const hash=b=>createHash('sha256').update(b).digest('hex');
const base=realpathSync(mkdtempSync(join(tmpdir(),'exhibitos-owned-execution-proof-')));chmodSync(base,0o700);
const checks=[],step=s=>{checks.push(s);console.log('PASS '+s);};
const files=['installed.json','engine.json','runtime.env','bundle/manifest.json','bundle/compose.yaml'];
const sourceHashes=()=>Object.fromEntries(files.map(n=>[n,hash(readFileSync(join(source,n)))]));
function inventory(p){return Object.fromEntries(readdirSync(p,{withFileTypes:true}).flatMap(e=>e.isDirectory()?Object.entries(inventory(join(p,e.name))).map(([n,h])=>[e.name+'/'+n,h]):[[e.name,hash(readFileSync(join(p,e.name)))]]));}
function containers(){const x=spawnSync('docker',['ps','-a','--format','{{.ID}} {{.Names}} {{.Image}} {{.State}}'],{encoding:'utf8'});assert.equal(x.status,0);return x.stdout.split('\n').filter(Boolean).sort();}
const originals=sourceHashes(),ciphertext=inventory(archive),keyHash=hash(readFileSync(keyFile)),beforeContainers=containers();
const kp=generateKeyPairSync('ed25519'),raw=kp.publicKey.export({type:'spki',format:'der'}).subarray(-32),now=Math.floor(Date.now()/1000);
const artifact=join(base,'runtime.tar'),data=Buffer.alloc(1024,0x73);writeFileSync(artifact,data,{mode:0o600});
const policy={format:1,channel:'development',target:'linux-arm64',protocolVersion:1,sourceSchemaSha256:'a'.repeat(64),minimumSequence:40,minimumIssuedAt:now-20,publicKeys:[raw.toString('hex')]};
const release={format:1,product:'ExhibitOS/runtime',channel:'development',target:'linux-arm64',version:'0.2.0-dev.2',sequence:41,issuedAt:now-10,expiresAt:now+3600,protocolVersion:1,sourceSchemas:['a'.repeat(64)],artifact:{name:'runtime.tar',bytes:data.length,sha256:hash(data),runtimeImageSha256:'b'.repeat(64),schemaSha256:'c'.repeat(64)}};
const payload=JSON.stringify(release),bytes=Buffer.from(payload),length=Buffer.alloc(8);length.writeBigUInt64BE(BigInt(bytes.length));
const envelope={format:1,algorithm:'ed25519',keyId:hash(raw),payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),length,bytes]),kp.privateKey).toString('hex')};
const put=(v,n)=>{const p=join(base,n+'.json');writeFileSync(p,JSON.stringify(v),{mode:0o600});return p;};
const policyFile=put(policy,'policy'),releaseFile=put(envelope,'release');
function call(profile,command,extra=[],error=null){const x=spawnSync(cli,[command,'--profile',profile,'--installation','default',...extra,'--apps-closed'],{encoding:'utf8',timeout:120000});assert.equal(x.error,undefined);assert.equal(x.stderr,'');const v=JSON.parse(x.stdout);if(error){assert.notEqual(x.status,0);assert.equal(v.code,error);}else assert.equal(x.status,0,JSON.stringify(v));return v;}
let n=0;
function setup(manifest=expected,registered=true,matching=true){const profile=join(base,'profile-'+n++);mkdirSync(profile,{mode:0o700});const id=randomUUID();
 call(profile,'trust-provision',['--policy',policyFile]);
 const plan={operationId:randomUUID(),sourceInstance:matching?id:randomUUID(),targetInstance:randomUUID(),sourceImage:'d'.repeat(64),targetImage:'b'.repeat(64),sourceSchema:'a'.repeat(64),targetSchema:'c'.repeat(64),backupId:'synthetic-backup',backupManifest:manifest,sourceInventory:'f'.repeat(64),requiredFreeBytes:1024};
 call(profile,'prepare-update',['--release',releaseFile,'--artifact',artifact,'--plan',put(plan,'plan-'+n)]);
 if(registered){putRegistry(profile,{format:1,activeId:id,installations:[{id,kind:'default',createdAt:0}]});const root=join(profile,'local-runtime');mkdirSync(root,{mode:0o700});mkdirSync(join(root,'bundle'),{mode:0o700});for(const f of files){const p=join(root,f);copyFileSync(join(source,f),p);chmodSync(p,0o600);}}
 return profile;
}
function putRegistry(p,r){writeFileSync(join(p,'installation-selection.json'),JSON.stringify(r),{mode:0o600});}
const missing=setup(expected,false);call(missing,'execution-status',[],'UPDATE_SOURCE_UNREGISTERED');step('missing registry refuses without creating an installation');
const foreign=setup(expected,true,false);call(foreign,'execution-status',[],'UPDATE_SOURCE_MISMATCH');step('foreign prepared source refuses before adapter execution');
const profile=setup();const observed=call(profile,'execution-status');assert.equal(observed.status.installed,true);assert.equal(observed.updateExecuted,false);step('registered source runs actual installed-service status under owned fence');
const extra=['--maintenance-image',image,'--key',keyFile,'--archive',archive];
const verified=call(profile,'verify-update-backup',extra);assert.equal(verified.verification.authenticatedManifestSha256,expected);assert.equal(verified.restoreVerified,false);assert.equal(verified.updateExecuted,false);step('actual Docker decrypts/authenticates the bound archive without claiming restoration or update');
const wrongPlan=setup('0'.repeat(64));call(wrongPlan,'verify-update-backup',extra,'UPDATE_BACKUP_MISMATCH');step('authenticated archive with foreign plaintext manifest refuses plan success');
call(profile,'verify-update-backup',['--maintenance-image','exhibitos:mutable','--key',keyFile,'--archive',archive],'BACKUP_IMAGE_INVALID');step('mutable maintenance image cannot authorize helper execution');
const wrongKey=join(base,'wrong-key.bin');writeFileSync(wrongKey,randomBytes(32),{mode:0o600});call(profile,'verify-update-backup',['--maintenance-image',image,'--key',wrongKey,'--archive',archive],'BACKUP_VERIFICATION_FAILED');step('actual wrong decryption key refuses and retains private failure candidate');
const marker=join(base,'legacy-lock-ready');const holder=spawn('python3',['-c','import fcntl,sys,time; f=open(sys.argv[1],"w"); fcntl.flock(f,fcntl.LOCK_EX); open(sys.argv[2],"w").write("ready"); time.sleep(20)',join(profile,'profile-session.lock'),marker],{stdio:'ignore'});
try{for(let i=0;i<200;i++){try{readFileSync(marker);break;}catch{}await new Promise(r=>setTimeout(r,25));}assert.equal(readFileSync(marker,'utf8'),'ready');chmodSync(join(profile,'profile-session.lock'),0o600);call(profile,'verify-update-backup',extra,'PROFILE_BUSY');}finally{holder.kill('SIGTERM');await new Promise(r=>holder.once('exit',r));}
step('actual independent legacy profile lock excludes owned execution');
const intent=call(profile,'update-intent');assert.equal(intent.trust.generation,2);assert.equal(intent.intent.update.stage,'prepared');assert.equal(intent.intent.update.preflight,null);step('all adapter observations leave prepared journal and preflight unchanged');
assert.deepEqual(sourceHashes(),originals);assert.deepEqual(inventory(archive),ciphertext);assert.equal(hash(readFileSync(keyFile)),keyHash);assert.deepEqual(containers(),beforeContainers);step('original installed fixture, encrypted archive, key and persistent container states preserved');
const report={format:1,checks,fixture:base,cliSha256:hash(readFileSync(cli)),verification:verified,observedState:observed.status.state,intentGeneration:intent.trust.generation,limits:['macOS Docker synthetic archive authentication only, not full update/migration/restore qualification','copied registered fixture refers to existing synthetic project; no global instance attestation','synthetic signed artifact is not an OCI import','no production data/native GUI/Windows/Podman validation; private signing key remains in memory']};
writeFileSync(join(base,'owned-execution-report.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:join(base,'owned-execution-report.json'),checks:checks.length}));
