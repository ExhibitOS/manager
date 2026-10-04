// SPDX-License-Identifier: Apache-2.0
// Only the explicitly supplied, retained synthetic bound-candidate fixture.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,realpathSync,chmodSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg,baseArg]=process.argv.slice(2);assert.ok(baseArg,'usage: node scripts/test-source-deployment.mjs <CLI> <retained synthetic bound-candidate workspace>');
const cli=resolve(cliArg),base=realpathSync(resolve(baseArg)),profile=join(base,'profile-1'),source=join(profile,'local-runtime');
const hash=b=>createHash('sha256').update(b).digest('hex'),checks=[],step=s=>{checks.push(s);console.log('PASS '+s);};
function call(p=profile,command='verify-update-source-deployment',flags=['--external-writers-quiesced'],error=null){const x=spawnSync(cli,[command,'--profile',p,'--installation','default',...flags,'--apps-closed'],{encoding:'utf8',timeout:120000});assert.equal(x.error,undefined);assert.equal(x.stderr,'');const v=JSON.parse(x.stdout);if(error){assert.notEqual(x.status,0);if(error==='UPDATE_USAGE')assert.ok(v.code.startsWith('UPDATE_USAGE: '));else assert.equal(v.code,error);}else assert.equal(x.status,0,JSON.stringify(v));return v;}
const intent=call(profile,'update-intent',[]);assert.equal(intent.trust.generation,2);assert.equal(intent.intent.update.stage,'prepared');assert.equal(intent.intent.update.preflight,null);
const target=join(profile,'installations',intent.intent.update.plan.targetInstance),job=JSON.parse(readFileSync(join(target,'restoration.json'))),workspace=join(target,'restore-'+job.id);
assert.equal(job.state,'completed');
const manifestPath=join(workspace,'authenticated/manifest.json'),receiptPath=join(workspace,'receipt.json'),registryPath=join(profile,'installation-selection.json'),envPath=join(source,'runtime.env');
const files=['installed.json','engine.json','runtime.env','bundle/manifest.json','bundle/compose.yaml'];
const snapshots=new Map([...files.map(f=>join(source,f)),manifestPath,receiptPath,registryPath].map(p=>[p,readFileSync(p)]));
function containers(){const x=spawnSync('docker',['ps','-a','--format','{{.ID}} {{.Names}} {{.Image}} {{.State}}'],{encoding:'utf8'});assert.equal(x.status,0);return x.stdout.split('\n').filter(Boolean).sort();}
const beforeStates=containers(),originalRoot='/private/tmp/exhibitos-manager-backup-create-5ld9x8zt/retained-source-manager',originalHashes=Object.fromEntries(files.map(f=>[f,hash(readFileSync(join(originalRoot,f)))]));
function changed(path,bytes,error){try{writeFileSync(path,bytes,{mode:0o600});call(profile,'verify-update-source-deployment',['--external-writers-quiesced'],error);}finally{writeFileSync(path,snapshots.get(path),{mode:0o600});chmodSync(path,0o600);}}
call(profile,'verify-update-source-deployment',[],'UPDATE_USAGE');step('missing external-writer acknowledgement refuses usage without accepting a proof');
const verified=call();assert.equal(verified.hostDeploymentVerified,true);assert.equal(verified.dataInventoryVerified,false);assert.equal(verified.preflightVerified,false);assert.equal(verified.updateExecuted,false);assert.equal(verified.observation.files.length,5);assert.equal(verified.observation.authenticatedManifestSha256,intent.intent.update.plan.backupManifest);step('actual owned stopped source five host files match exact authenticated registered candidate backup');
changed(envPath,Buffer.concat([snapshots.get(envPath),Buffer.from('\n# synthetic change after backup\n')]),'UPDATE_SOURCE_DEPLOYMENT_MISMATCH');step('actual current source environment byte change refuses without engine or journal transition');
changed(manifestPath,Buffer.concat([snapshots.get(manifestPath),Buffer.from(' ')]),'UPDATE_RESTORE_BINDING_MISMATCH');step('raw candidate authenticated manifest tamper refuses the stored plan hash');
const receipt=JSON.parse(snapshots.get(receiptPath));receipt.sourceVerification.runtimeImageSha256='0'.repeat(64);changed(receiptPath,Buffer.from(JSON.stringify(receipt)),'UPDATE_RESTORE_BINDING_MISMATCH');step('foreign candidate source Runtime proof refuses before current-source comparison');
call(join(base,'profile-0'),'verify-update-source-deployment',['--external-writers-quiesced'],'UPDATE_SOURCE_PROOF_MISSING');step('retained failed restoration cannot supply host deployment proof');
const registry=JSON.parse(snapshots.get(registryPath));registry.installations=registry.installations.filter(e=>e.id!==intent.intent.update.plan.targetInstance);changed(registryPath,Buffer.from(JSON.stringify(registry)),'UPDATE_TARGET_UNREGISTERED');step('missing registered planned target refuses without deleting or recreating its root');
const repeated=call();assert.deepEqual(repeated.observation.files,verified.observation.files);step('restored private test bytes produce the same file proof on a new CLI observation');
for(const [p,b] of snapshots)assert.equal(hash(readFileSync(p)),hash(b));assert.deepEqual(containers(),beforeStates);assert.deepEqual(Object.fromEntries(files.map(f=>[f,hash(readFileSync(join(originalRoot,f)))])),originalHashes);const after=call(profile,'update-intent',[]);assert.deepEqual(after,intent);step('original deployment, candidate bytes/receipt, selection, persistent containers and complete Prepared intent retained');
const report={format:1,checks,fixture:base,cliSha256:hash(readFileSync(cli)),observation:verified.observation,limits:['five host deployment files only, no DB/blob/config-volume current equality or full preflight','copied synthetic registered source shares an existing Docker project; not global runtime isolation qualification','test temporarily changes only owned synthetic source/manifest/receipt/registry copies then restores exact bytes; no original root/data/keys/archives/volumes changed','no new helper, full restoration, source start/stop, update/migration/rollback/nativeGUI/Windows/Podman execution']};
const reportPath=join(base,'source-deployment-report.json');writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:reportPath,checks:checks.length}));
