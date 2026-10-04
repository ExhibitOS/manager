// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,chmodSync,realpathSync,createReadStream,lstatSync} from 'node:fs';
import {join,resolve,basename} from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const [cliArg,baseArg,resultArg,inventoryArg]=process.argv.slice(2);assert.ok(inventoryArg,'usage: <CLI> <retained fixture> <actual configuration JSON> <earlier actual inventory JSON>');
const cli=resolve(cliArg),base=realpathSync(baseArg),profile=join(base,'profile-1'),actual=JSON.parse(readFileSync(resultArg)),previous=JSON.parse(readFileSync(inventoryArg));
assert.equal(actual.configurationInventoryVerified,true);assert.equal(actual.imageBytesVerified,true);assert.equal(actual.preflightVerified,false);assert.equal(actual.updateExecuted,false);assert.deepEqual(actual.intent,previous.intent);
const o=actual.observation;assert.equal(o.files.length,7);assert.equal(new Set(o.files.map(f=>f.name)).size,7);assert.equal(o.images.length,2);assert.equal(o.authenticatedManifestSha256,actual.intent.update.plan.backupManifest);
const checks=[],step=s=>{checks.push(s);console.log('PASS '+s);};
for(const image of o.images){const p=join(o.exportWorkspace,basename(image.archive)),s=lstatSync(p);assert.ok(s.isFile()&&!s.isSymbolicLink()&&s.nlink===1&&(s.mode&0o7777)===0o600&&s.uid===process.getuid());assert.equal(s.size,image.bytes);const sha=createHash('sha256');for await(const b of createReadStream(p))sha.update(b);assert.equal(sha.digest('hex'),image.sha256);}
step('actual exact7 current configurations plus newly exported2 complete image byte hashes match authenticated backup');
function docker(...args){const r=spawnSync('docker',args,{encoding:'utf8'});assert.equal(r.status,0);return r.stdout;}
function call(command,p=profile,flags=[],error=null){const r=spawnSync(cli,[command,'--profile',p,'--installation','default',...flags,'--apps-closed'],{encoding:'utf8',timeout:120000});assert.equal(r.error,undefined);assert.equal(r.stderr,'');const v=JSON.parse(r.stdout);if(error){assert.notEqual(r.status,0);if(error==='UPDATE_USAGE')assert.ok(v.code.startsWith('UPDATE_USAGE:'));else assert.equal(v.code,error);}else assert.equal(r.status,0);return v;}
const states=docker('ps','-a','--format','{{.ID}} {{.State}}'),volumes=docker('volume','ls','--format','{{.Name}}'),intent=call('update-intent');assert.deepEqual(intent.intent,actual.intent);
const flags=['--image','sha256:8f0e7b042ff0b93a646b919f5a8a5ee2f41cc22debcd5bd9ef49eacd06537e06','--external-writers-quiesced'];
call('verify-update-configuration-inventory',profile,flags.slice(0,2),'UPDATE_USAGE');call('verify-update-configuration-inventory',join(base,'profile-0'),flags,'UPDATE_SOURCE_PROOF_MISSING');step('missing acknowledgement and failed registered candidate refuse before exporting or creating helpers');
const target=join(profile,'installations',actual.intent.update.plan.targetInstance),job=JSON.parse(readFileSync(join(target,'restoration.json'))),manifest=join(target,'restore-'+job.id,'authenticated/manifest.json'),raw=readFileSync(manifest);
try{writeFileSync(manifest,Buffer.concat([raw,Buffer.from(' ')]));call('verify-update-configuration-inventory',profile,flags,'UPDATE_RESTORE_BINDING_MISMATCH');}finally{writeFileSync(manifest,raw);chmodSync(manifest,0o600);}
const env=join(profile,'local-runtime/runtime.env'),bytes=readFileSync(env);
try{writeFileSync(env,Buffer.concat([bytes,Buffer.from('\n# owned synthetic configuration test\n')]));call('verify-update-configuration-inventory',profile,flags,'UPDATE_SOURCE_DEPLOYMENT_MISMATCH');}finally{writeFileSync(env,bytes);chmodSync(env,0o600);}
assert.deepEqual(readFileSync(manifest),raw);assert.deepEqual(readFileSync(env),bytes);assert.deepEqual(call('update-intent'),intent);assert.equal(docker('ps','-a','--format','{{.ID}} {{.State}}'),states);assert.equal(docker('volume','ls','--format','{{.Name}}'),volumes);step('raw manifest/current source change refuses; original restored bytes, containers/volumes and full Prepared preserved');
const out=join(base,'full-configuration-report.json');writeFileSync(out,JSON.stringify({format:1,checks,observation:o,limits:['complete supported7file configuration and exact current Engine exports, not global privileged-writer isolation/full preflight','Docker image save byte serialization must match archived version or refuse; artifacts/partial files retained outside Git','resource/compatibility/current coherent security-state recovery and signed apply/migration/health/rollback/native GUI/Windows/device remain']},null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:out,checks:checks.length}));
