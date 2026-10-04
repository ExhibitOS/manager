// SPDX-License-Identifier: Apache-2.0
// Only supplied retained synthetic plan; preserve full Prepared and source state.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,chmodSync,realpathSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg,baseArg,resultArg,copyArg]=process.argv.slice(2);assert.ok(copyArg,'usage: <CLI> <retained candidate> <actual inventory result> <earlier actual physical copy result>');
const cli=resolve(cliArg),base=realpathSync(baseArg),profile=join(base,'profile-1'),actual=JSON.parse(readFileSync(resultArg)),copy=JSON.parse(readFileSync(copyArg));
assert.equal(actual.dataInventoryVerified,true);assert.equal(actual.configurationInventoryVerified,false);assert.equal(actual.preflightVerified,false);assert.equal(actual.updateExecuted,false);
const o=actual.observation,plan=actual.intent.update.plan;
assert.equal(o.inventory.inventorySha256,plan.sourceInventory);assert.equal(o.inventory.schemaSha256,plan.sourceSchema);assert.equal(o.inventory.authenticatedManifestSha256,plan.backupManifest);assert.equal(o.inventory.backupId,plan.backupId);
assert.notEqual(o.snapshotVolume,o.repeatedSnapshotVolume);assert.equal(o.sourceContentSha256,copy.observation.proof.contentSha256);assert.equal(o.sourceDatabaseVolume,copy.observation.sourceVolume);
const checks=['actual borrowed-fence isolated DB and readonly original blob inventory matches exact authenticated plan','fresh post-comparison physical source copy matches earlier source content; original DB never started'],step=s=>{checks.push(s);console.log('PASS '+s);};
function call(command,p=profile,flags=[],error=null){const r=spawnSync(cli,[command,'--profile',p,'--installation','default',...flags,'--apps-closed'],{encoding:'utf8',timeout:120000});assert.equal(r.error,undefined);assert.equal(r.stderr,'');const v=JSON.parse(r.stdout);if(error){assert.notEqual(r.status,0);if(error==='UPDATE_USAGE')assert.ok(v.code.startsWith('UPDATE_USAGE:'));else assert.equal(v.code,error);}else assert.equal(r.status,0);return v;}
function docker(...args){const r=spawnSync('docker',args,{encoding:'utf8'});assert.equal(r.status,0);return r.stdout;}
const containers=docker('ps','-a','--format','{{.ID}} {{.State}}'),volumes=docker('volume','ls','--format','{{.Name}}'),intent=call('update-intent');assert.deepEqual(intent.intent,actual.intent);
const flags=['--image',copy.observation.maintenanceImage,'--external-writers-quiesced'];
call('verify-update-source-inventory',profile,flags.slice(0,2),'UPDATE_USAGE');call('verify-update-source-inventory',join(base,'profile-0'),flags,'UPDATE_SOURCE_PROOF_MISSING');step('missing operator acknowledgement and failed registered candidate refuse without allocating copies');
const env=join(profile,'local-runtime/runtime.env'),bytes=readFileSync(env);
try{writeFileSync(env,Buffer.concat([bytes,Buffer.from('\n# owned synthetic inventory test\n')]));call('verify-update-source-inventory',profile,flags,'UPDATE_SOURCE_DEPLOYMENT_MISMATCH');}finally{writeFileSync(env,bytes);chmodSync(env,0o600);}
assert.deepEqual(readFileSync(env),bytes);assert.deepEqual(call('update-intent'),intent);assert.equal(docker('ps','-a','--format','{{.ID}} {{.State}}'),containers);assert.equal(docker('volume','ls','--format','{{.Name}}'),volumes);step('current source change refuses before Engine writes; original state and complete Prepared intent retained');
const out=join(base,'source-inventory-report.json');writeFileSync(out,JSON.stringify({format:1,checks,observation:o,limits:['observations plus operator acknowledgement, not global privileged-writer isolation or durable full preflight','complete configuration/image bytes/compatibility/security-state recovery and signed apply/migration/health/rollback still required','snapshot copies/private data retained outside Git; native PostgreSQL18/UID999 and same maintenance image only']},null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:out,checks:checks.length}));
