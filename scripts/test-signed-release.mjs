// SPDX-License-Identifier: Apache-2.0
// Real Node/OpenSSL Ed25519 -> Rust CLI interoperability, synthetic fixtures only.
import assert from 'node:assert/strict';
import {generateKeyPairSync, sign, createHash} from 'node:crypto';
import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, chmodSync, symlinkSync, linkSync, statSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
const cli=resolve(process.argv[2]??'target/release/exhibitos-update');
const base=mkdtempSync(join(tmpdir(),'exhibitos-signed-proof-'));chmodSync(base,0o700);
const checks=[];
const hash=b=>createHash('sha256').update(b).digest('hex');
const {publicKey,privateKey}=generateKeyPairSync('ed25519');
const raw=publicKey.export({type:'spki',format:'der'}).subarray(-32);
const id=hash(raw),now=Math.floor(Date.now()/1000),artifact=join(base,'runtime.tar');
const data=Buffer.alloc(2*1024*1024,0x7b);data.set(Buffer.from('SYNTHETIC SIGNED RELEASE, NOT A DEPLOYABLE OCI BUNDLE'));
writeFileSync(artifact,data,{mode:0o600});
const source='a'.repeat(64),image='b'.repeat(64),schema='c'.repeat(64);
const policy={format:1,channel:'development',target:'linux-arm64',protocolVersion:1,sourceSchemaSha256:source,minimumSequence:40,minimumIssuedAt:now-10,publicKeys:[raw.toString('hex')]};
const release={format:1,product:'ExhibitOS/runtime',channel:'development',target:'linux-arm64',version:'0.2.0-dev.1',sequence:41,issuedAt:now-5,expiresAt:now+3600,protocolVersion:1,sourceSchemas:[source],artifact:{name:'runtime.tar',bytes:data.length,sha256:hash(data),runtimeImageSha256:image,schemaSha256:schema}};
let n=0;
function put(value,name='fixture'){const p=join(base,`${name}-${n++}.json`);writeFileSync(p,JSON.stringify(value),{mode:0o600});return p;}
function envelope(r=release,k=privateKey,keyId=id){const payload=typeof r==='string'?r:JSON.stringify(r),bytes=Buffer.from(payload),len=Buffer.alloc(8);len.writeBigUInt64BE(BigInt(bytes.length));return {format:1,algorithm:'ed25519',keyId,payload,signature:sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),len,bytes]),k).toString('hex')};}
const policyPath=put(policy,'trusted-policy'),releasePath=put(envelope(),'signed-release');
function call({p=policyPath,r=releasePath,f=artifact,error=null}={}){const x=spawnSync(cli,['verify','--policy',p,'--release',r,'--artifact',f],{encoding:'utf8',timeout:90000});assert.equal(x.stderr,'');assert.equal(x.error,undefined);const v=JSON.parse(x.stdout);if(error){assert.notEqual(x.status,0);assert.equal(v.code,error);}else {assert.equal(x.status,0,JSON.stringify(v));assert.equal(v.signatureVerified,true);assert.equal(v.artifactVerified,true);assert.equal(v.activated,false);assert.equal(v.backupRestoreVerified,false);}return v;}
function step(s){checks.push(s);console.log('PASS '+s);}
const receipt=call();assert.equal(receipt.artifactSha256,hash(data));assert.equal(receipt.keyId,id);step('independent Node Ed25519 signs exact domain/length/UTF8 bytes; Rust verifies actual 2MiB artifact');
const tampered=envelope();tampered.payload+=' ';call({r:put(tampered),error:'UPDATE_SIGNATURE_INVALID'});
const cross=envelope();cross.signature=sign(null,Buffer.from(cross.payload),privateKey).toString('hex');call({r:put(cross),error:'UPDATE_SIGNATURE_INVALID'});
const other=generateKeyPairSync('ed25519');const foreign=hash(other.publicKey.export({type:'spki',format:'der'}).subarray(-32));call({r:put(envelope(release,other.privateKey,foreign)),error:'UPDATE_KEY_UNTRUSTED'});step('tampering, missing signature domain and feed-supplied foreign signing key refuse');
call({p:put({...policy,minimumSequence:41}),error:'UPDATE_RELEASE_REPLAY'});call({p:put({...policy,minimumIssuedAt:now}),error:'UPDATE_RELEASE_REPLAY'});
call({r:put(envelope({...release,expiresAt:now-1})),error:'UPDATE_RELEASE_EXPIRED'});call({r:put(envelope({...release,issuedAt:now+3600,expiresAt:now+7200})),error:'UPDATE_RELEASE_EXPIRED'});step('monotonic sequence/time floor, expiry and future issue time refuse');
for(const change of [{channel:'stable'},{target:'linux-amd64'},{sourceSchemaSha256:'d'.repeat(64)}])call({p:put({...policy,...change}),error:'UPDATE_POLICY_MISMATCH'});step('pinned channel, target architecture and source schema match required');
const payload=JSON.stringify(release);call({r:put(envelope(payload.replace('{','{"format":1,'))),error:'UPDATE_RELEASE_INVALID'});call({r:put(envelope({...release,extra:true})),error:'UPDATE_RELEASE_INVALID'});call({r:put(envelope({...release,artifact:{...release.artifact,name:'../runtime.tar'}})),error:'UPDATE_RELEASE_INVALID'});step('authenticated duplicate/unknown fields and artifact path escape refuse');
const badDir=join(base,'changed');mkdirSync(badDir,{mode:0o700});const changed=join(badDir,'runtime.tar');writeFileSync(changed,Buffer.from('changed'),{mode:0o600});call({f:changed,error:'UPDATE_ARTIFACT_MISMATCH'});
writeFileSync(changed,Buffer.alloc(data.length,0),{mode:0o600});call({f:changed,error:'UPDATE_ARTIFACT_MISMATCH'});step('actual short and same-length modified artifact bytes fail size/hash validation');
const unsafe=put(policy,'unsafe-policy');chmodSync(unsafe,0o622);call({p:unsafe,error:'UPDATE_POLICY_UNTRUSTED'});
const alias=join(base,'policy-alias');symlinkSync(policyPath,alias);call({p:alias,error:'UPDATE_INPUT_UNAVAILABLE'});
const linkedDir=join(base,'linked');mkdirSync(linkedDir,{mode:0o700});const linked=join(linkedDir,'runtime.tar');linkSync(artifact,linked);call({f:linked,error:'UPDATE_INPUT_INVALID'});step('unsafe writable policy, symlink input and multiply-linked artifact refuse without mutations');
assert.equal(hash(readFileSync(artifact)),hash(data));assert.equal(statSync(artifact).nlink,2);assert.equal(readFileSync(policyPath,'utf8'),JSON.stringify(policy));assert.equal(readFileSync(releasePath,'utf8'),JSON.stringify(envelope()));step('source artifact, policy and signed envelope retained; signing secret never serialized');
const report={format:1,checks,fixture:base,receipt,cliSha256:hash(readFileSync(cli)),limits:['offline verifier only; synthetic artifact is not a deployable OCI bundle','no policy-floor persistence, production trust root, download, engine mutation, backup/migration/activation or release signing account','source and all malformed fixtures retained; private signing key existed only in process memory']};const reportPath=join(base,'signed-release-report.json');writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log('Report '+reportPath);console.log('SHA256 '+hash(readFileSync(reportPath)));
