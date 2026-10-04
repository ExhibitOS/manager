// SPDX-License-Identifier: Apache-2.0
// Existing retained synthetic candidate only; no original volume mutation.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,chmodSync,mkdtempSync,mkdirSync,linkSync,symlinkSync,readdirSync,realpathSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
const [cliArg,baseArg,receiptArg]=process.argv.slice(2);assert.ok(receiptArg,'usage: node test-source-database.mjs <CLI> <retained candidate fixture> <actual snapshot CLI result>');
const cli=resolve(cliArg),base=realpathSync(baseArg),profile=join(base,'profile-1');
const actual=JSON.parse(readFileSync(receiptArg)),proof=actual.observation.proof;
assert.equal(actual.physicalDatabaseCopied,true);assert.equal(actual.dataInventoryVerified,false);assert.equal(actual.preflightVerified,false);assert.equal(actual.updateExecuted,false);assert.equal(proof.cleanShutdown,true);assert.ok(proof.files>100 && proof.bytes>1024*1024);
const checks=[],step=s=>{checks.push(s);console.log('PASS '+s);};
step('actual full owned CLI snapshot copied native PostgreSQL18 bytes/permissions/owners with source census unchanged and clean shutdown verified');
function call(command,p=profile,flags=[],error=null){const r=spawnSync(cli,[command,'--profile',p,'--installation','default',...flags,'--apps-closed'],{encoding:'utf8',timeout:120000});assert.equal(r.error,undefined);assert.equal(r.stderr,'');const v=JSON.parse(r.stdout);if(error){assert.notEqual(r.status,0);if(error==='UPDATE_USAGE')assert.ok(v.code.startsWith('UPDATE_USAGE:'));else assert.equal(v.code,error);}else assert.equal(r.status,0);return v;}
function docker(...args){const r=spawnSync('docker',args,{encoding:'utf8'});assert.equal(r.status,0);return r.stdout;}
const volumes=docker('volume','ls','--format','{{.Name}}'),containers=docker('ps','-a','--format','{{.ID}} {{.State}}');
const intent=call('update-intent');assert.deepEqual(intent.intent,actual.intent);
const flags=['--image',actual.observation.maintenanceImage,'--external-writers-quiesced'];
call('snapshot-update-source-database',profile,flags.slice(0,2),'UPDATE_USAGE');
call('snapshot-update-source-database',join(base,'profile-0'),flags,'UPDATE_SOURCE_PROOF_MISSING');
step('missing operator acknowledgement and failed registered candidate refuse before creating a snapshot');
const env=join(profile,'local-runtime/runtime.env'),original=readFileSync(env);
try{writeFileSync(env,Buffer.concat([original,Buffer.from('\n# owned synthetic change\n')]));call('snapshot-update-source-database',profile,flags,'UPDATE_SOURCE_DEPLOYMENT_MISMATCH');}finally{writeFileSync(env,original);chmodSync(env,0o600);}
assert.deepEqual(readFileSync(env),original);assert.equal(docker('volume','ls','--format','{{.Name}}'),volumes);assert.equal(docker('ps','-a','--format','{{.ID}} {{.State}}'),containers);assert.deepEqual(call('update-intent'),intent);
step('changed owned source deployment refuses before Engine writes; file bytes, volumes, containers and complete Prepared intent retained');
const script=readFileSync('crates/lifecycle/src/source_database_copy.mjs','utf8');
for(const kind of ['hardlink','symlink','fifo']){
 const root=realpathSync(mkdtempSync(join(tmpdir(),'exhibitos-db-copy-refusal-'))),src=join(root,'source'),dest=join(root,'snapshot');mkdirSync(src,{mode:0o700});mkdirSync(dest,{mode:0o700});
 writeFileSync(join(src,'file'),'synthetic',{mode:0o600});
 if(kind==='hardlink')linkSync(join(src,'file'),join(src,'alias'));
 if(kind==='symlink')symlinkSync('file',join(src,'alias'));
 if(kind==='fifo')assert.equal(spawnSync('mkfifo',[join(src,'pipe')]).status,0);
 const local=script.replace("const source='/source',target='/snapshot'",'const source='+JSON.stringify(src)+',target='+JSON.stringify(dest));
 const r=spawnSync(process.execPath,['--input-type=module','-e',local],{encoding:'utf8',timeout:10000});assert.equal(r.error,undefined);assert.equal(r.status,1);assert.equal(r.stdout,'');assert.match(r.stderr,/SOURCE_DATABASE_COPY_REFUSED:source-census/);assert.deepEqual(readdirSync(dest),[]);
 step(kind+' source refuses before copying; synthetic negative fixture retained');
}
const report={format:1,checks,fixture:base,cliSha256:createHash('sha256').update(readFileSync(cli)).digest('hex'),observation:actual.observation,limits:['physical copy only; logical current database/blob/configuration inventory comparison remains','PostgreSQL18 clean-shutdown/native-volume layout only; tablespaces, links, special files, greater than2GiB layouts refuse','external privileged writers and later restarts are not globally fenced; no signed apply/migration/health/rollback','failed and successful snapshot volumes/private fixtures retained; no user data or archives deleted']};
const out=join(base,'source-database-report.json');writeFileSync(out,JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify({report:out,checks:checks.length}));
