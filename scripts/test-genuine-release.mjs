// SPDX-License-Identifier: Apache-2.0
// Local development qualification only. No production keys or engine mutation.
import assert from 'node:assert/strict';
import {generateKeyPairSync,sign,createHash} from 'node:crypto';
import {readFileSync,writeFileSync,copyFileSync,mkdirSync,statSync,openSync,readSync,writeSync,closeSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg,baseArg,proofArg]=process.argv.slice(2);
assert(cliArg&&baseArg,'usage: node scripts/test-genuine-release.mjs <CLI> <private OCI qualification workspace>');
const cli=resolve(cliArg),base=resolve(baseArg),artifact=join(base,'runtime.tar');
const proofPath=proofArg?resolve(proofArg):join(base,'oci-proof.json');
const proof=JSON.parse(readFileSync(proofPath,'utf8')); 
assert.equal(proof.format,1);assert.equal(proof.target,'linux-arm64');
assert.equal(statSync(artifact).size,proof.artifactBytes);
assert(proof.artifactBytes>1024*1024&&/^[a-f0-9]{64}$/.test(proof.runtimeImageSha256));
const hash=b=>createHash('sha256').update(b).digest('hex');
function fileHash(path){const h=createHash('sha256'),fd=openSync(path,'r'),b=Buffer.alloc(65536);try{let n;while((n=readSync(fd,b,0,b.length,null)))h.update(b.subarray(0,n));}finally{closeSync(fd);}return h.digest('hex');}
assert.equal(fileHash(artifact),proof.artifactSha256);
const run=join(base,'signature-'+Date.now());mkdirSync(run,{mode:0o700});
const {privateKey,publicKey}=generateKeyPairSync('ed25519');
const raw=publicKey.export({type:'spki',format:'der'}).subarray(-32),id=hash(raw),now=Math.floor(Date.now()/1000);
const policy={format:1,channel:'development',target:proof.target,protocolVersion:1,sourceSchemaSha256:proof.sourceSchemaSha256,minimumSequence:40,minimumIssuedAt:now-10,publicKeys:[raw.toString('hex')]};
const release={format:1,product:'ExhibitOS/runtime',channel:'development',target:proof.target,version:proof.version,sequence:41,issuedAt:now-5,expiresAt:now+3600,protocolVersion:1,sourceSchemas:[proof.sourceSchemaSha256],artifact:{name:'runtime.tar',bytes:proof.artifactBytes,sha256:proof.artifactSha256,runtimeImageSha256:proof.runtimeImageSha256,schemaSha256:proof.targetSchemaSha256}};
function envelope(value){const payload=JSON.stringify(value),bytes=Buffer.from(payload),length=Buffer.alloc(8);length.writeBigUInt64BE(BigInt(bytes.length));return {format:1,algorithm:'ed25519',keyId:id,payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),length,bytes]),privateKey).toString('hex')};}
let n=0;function put(value){const p=join(run,`input-${n++}.json`);writeFileSync(p,JSON.stringify(value),{mode:0o600,flag:'wx'});return p;}
const policyPath=put(policy),envelopeValue=envelope(release),releasePath=put(envelopeValue),checks=[];
function call(p=policyPath,r=releasePath,f=artifact,error=null){const x=spawnSync(cli,['verify','--policy',p,'--release',r,'--artifact',f],{encoding:'utf8',timeout:90000});assert.equal(x.error,undefined);assert.equal(x.stderr,'');const v=JSON.parse(x.stdout);if(error){assert.notEqual(x.status,0);assert.equal(v.code,error);}else{assert.equal(x.status,0,JSON.stringify(v));assert.equal(v.signatureVerified,true);assert.equal(v.artifactVerified,true);assert.equal(v.activated,false);assert.equal(v.backupRestoreVerified,false);}return v;}
const receipt=call();assert.equal(receipt.artifactSha256,proof.artifactSha256);checks.push('Rust verifies independent Ed25519 envelope and full genuine OCI archive bytes');
call(policyPath,put({...envelopeValue,payload:envelopeValue.payload+' '}),artifact,'UPDATE_SIGNATURE_INVALID');checks.push('changed signed payload refused');
for(const change of [{target:'linux-amd64'},{sourceSchemaSha256:'d'.repeat(64)}])call(put({...policy,...change}),releasePath,artifact,'UPDATE_POLICY_MISMATCH');checks.push('foreign architecture and source schema refused');
call(put({...policy,minimumSequence:41}),releasePath,artifact,'UPDATE_RELEASE_REPLAY');checks.push('explicit sequence floor refused; persistence not tested');
const changed=join(run,'runtime.tar');copyFileSync(artifact,changed);const fd=openSync(changed,'r+');try{const byte=Buffer.alloc(1);assert.equal(readSync(fd,byte,0,1,4096),1);byte[0]^=1;assert.equal(writeSync(fd,byte,0,1,4096),1);}finally{closeSync(fd);}
assert.equal(statSync(changed).size,proof.artifactBytes);call(policyPath,releasePath,changed,'UPDATE_ARTIFACT_MISMATCH');checks.push('same-size one-byte modified archive refused');
assert.equal(fileHash(artifact),proof.artifactSha256);assert.equal(readFileSync(policyPath,'utf8'),JSON.stringify(policy));assert.equal(readFileSync(releasePath,'utf8'),JSON.stringify(envelopeValue));checks.push('original artifact and development policy/envelope preserved');
const report={format:1,checks,receipt,artifactBytes:proof.artifactBytes,artifactSha256:proof.artifactSha256,runtimeImageSha256:proof.runtimeImageSha256,cliSha256:fileHash(cli),validatorProofSha256:fileHash(proofPath),limits:['ephemeral process-memory development private key; separate local fixture policy, not existing deployment trust authority','no OCI import/start, actual compatibility, migration, health, activation or rollback','no durable floor, revocation or coherent backup/security-state recovery validation','original and malformed artifacts retained privately']};
writeFileSync(join(run,'report.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600,flag:'wx'});
for(const check of checks)console.log('PASS '+check);console.log('Report '+join(run,'report.json'));
