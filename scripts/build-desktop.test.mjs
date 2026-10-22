// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,readFile,symlink,link,realpath,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {buildDesktop,officialIntent,desktopFailure} from './build-desktop.mjs';
import {QUALIFIED_REDISTRIBUTION,DISTRIBUTION_FILES} from './update-runtime-distribution.mjs';
import {SOURCE} from './prepare-update-runtime.mjs';
const manifest={format:'exhibitos-manager-update-runtime-v1',version:1,redistribution:'private-validation-only'};
const official={APPLE_SIGNING_IDENTITY:'Developer ID Application: Synthetic unused identity'};
async function fixture(t,value=manifest){
 const repo=await mkdtemp(join(await realpath(tmpdir()),'exhi-desktop-boundary-'));t.after(()=>rm(repo,{recursive:true,force:true}));
 await mkdir(join(repo,'src-tauri/update-runtime/generated'),{recursive:true});await writeFile(join(repo,'src-tauri/tauri.conf.json'),'{}');
 if(value!==null)await writeFile(join(repo,'src-tauri/update-runtime/generated/manifest.json'),typeof value==='string'?value:JSON.stringify(value));
 return repo;
}
async function cliFixture(t){
 const repo=await fixture(t);
 await mkdir(join(repo,'scripts/update-runtime-notices'),{recursive:true});
 for(const name of ['build-desktop.mjs','official-runtime-guard.mjs','official-mac-distribution.mjs','update-runtime-distribution.mjs','prepare-update-runtime.mjs','update-runtime-archive.mjs','update-runtime-notices/distribution-files.json','update-runtime-notices/DISTRIBUTION.txt']){
  await writeFile(join(repo,'scripts',name),await readFile(new URL(name,import.meta.url)));
 }
 return join(repo,'scripts/build-desktop.mjs');
}
test('local default, adhoc and named Apple Development keep the existing two commands',async t=>{
 const repo=await fixture(t,null);
 for(const environment of [{},{APPLE_SIGNING_IDENTITY:'-'},{APPLE_SIGNING_IDENTITY:'Apple Development: Synthetic'}]){
  const commands=[];const result=await buildDesktop({repo,environment},{runCommand:async command=>commands.push(command)});
  assert.deepEqual(commands.map(c=>c.args),[['run','remote:prepare'],[join(repo,'node_modules/@tauri-apps/cli/tauri.js'),'build','--bundles','app']]);assert.equal(result.officialReleaseQualified,false);
 }
});
test('read-only plan does not run preparation or Tauri',async t=>{
 const repo=await fixture(t);let calls=0;const result=await buildDesktop({repo,environment:{},planOnly:true},{runCommand:()=>calls++});assert.equal(calls,0);assert.equal(result.buildExecuted,false);
});
test('official environment or source config refuses private runtime before either command',async t=>{
 const repo=await fixture(t);let calls=0;
 for(const environment of [official,{APPLE_CERTIFICATE:'synthetic-unused'},{APPLE_CERTIFICATE_PASSWORD:'synthetic-unused'},{APPLE_API_KEY:'synthetic-unused'},{APPLE_ID:'synthetic-unused'},{TAURI_SIGNING_PRIVATE_KEY:'synthetic-unused'},{APPLE_SIGNING_IDENTITY:'OPAQUE_SYNTHETIC_SHA1'},{TAURI_CONFIG:'synthetic-unused'}]){
  await assert.rejects(buildDesktop({repo,environment},{runCommand:()=>calls++}),/DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED/);
 }
 await writeFile(join(repo,'src-tauri/tauri.conf.json'),JSON.stringify({bundle:{macOS:{signingIdentity:'Developer ID Application: Synthetic'}}}));
 await assert.rejects(buildDesktop({repo,environment:{}},{runCommand:()=>calls++}),/DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED/);assert.equal(calls,0);
 assert.equal(officialIntent({APPLE_SIGNING_IDENTITY:'Apple Development: Synthetic',APPLE_API_ISSUER:'synthetic'},{}),true);
 assert.equal(officialIntent({},{bundle:{createUpdaterArtifacts:'v1Compatible'}}),true);
});
test('missing, malformed, unknown and relabelled public manifests cannot enable official build',async t=>{
 for(const value of [null,'{not-json',{}, {...manifest,redistribution:'public'}, {...manifest,redistribution:'approved'}, {...manifest,version:2}]){
  const repo=await fixture(t,value);let calls=0;
  await assert.rejects(buildDesktop({repo,environment:official},{runCommand:()=>calls++}),value===null?/DESKTOP_RELEASE_RUNTIME_MISSING/:/DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED/);assert.equal(calls,0);
 }
});
test('symlink parent/file, hardlink and oversized descriptor refuse without mutation',async t=>{
 for(const kind of ['parent','file','hardlink','oversized']){
  const repo=await fixture(t,null),folder=join(repo,'src-tauri/update-runtime/generated'),file=join(folder,'manifest.json'),external=join(repo,'synthetic-external.json');
  await writeFile(external,JSON.stringify(manifest));
  if(kind==='parent'){await rm(folder,{recursive:true});await symlink(repo,folder,'dir');}
  else if(kind==='file')await symlink(external,file);
  else if(kind==='hardlink')await link(external,file);
  else await writeFile(file,Buffer.alloc(2*1024**2+1));
  let calls=0;await assert.rejects(buildDesktop({repo,environment:official},{runCommand:()=>calls++}),kind==='oversized'?/DESKTOP_RELEASE_RUNTIME_TOO_LARGE/:/DESKTOP_RELEASE_RUNTIME_UNSAFE/);assert.equal(calls,0);assert.equal(await readFile(external,'utf8'),JSON.stringify(manifest));
 }
});
test('actual desktop CLI refuses official intent before remote prepare and hides credential values',async t=>{
 const cli=await cliFixture(t);
 const secret='synthetic-must-not-be-logged',result=spawnSync(process.execPath,[cli,'--plan'],{encoding:'utf8',env:{...process.env,APPLE_SIGNING_IDENTITY:official.APPLE_SIGNING_IDENTITY,APPLE_PASSWORD:secret},timeout:10000});
 assert.equal(result.status,1);const report=JSON.parse(result.stderr);assert.equal(report.code,'DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED');assert.equal(report.buildExecuted,false);assert.equal(result.stdout,'');assert.equal(result.stderr.includes(secret),false);assert.equal(result.stderr.includes(official.APPLE_SIGNING_IDENTITY),false);
});

test('ordinary Tauri/Cargo arguments retain exact order; configuration overrides refuse before preparation',async t=>{
 const repo=await fixture(t),args=['--debug','--target','aarch64-apple-darwin','--','--locked','--offline'],commands=[];
 await buildDesktop({repo,environment:{},args},{runCommand:async command=>commands.push(command)});
 assert.deepEqual(commands[1].args.slice(4),args);assert.equal(commands[1].shell,false);
 for(const override of [['--config','synthetic.json'],['-c','synthetic.json'],['--config=synthetic.json'],['-csynthetic.json'],['-cx']]){
  let calls=0;await assert.rejects(buildDesktop({repo,environment:{},args:override},{runCommand:()=>calls++}),/DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED/);assert.equal(calls,0);
 }
});
test('failure reports distinguish prebuild, preparation and Tauri command execution without argument values',async t=>{
 const repo=await fixture(t),secret='synthetic-error-detail-must-not-be-logged';
 for(const failedIndex of [0,1]){
  let calls=0;
  try{await buildDesktop({repo,environment:{},args:['--debug']},{runCommand:async()=>{if(calls++===failedIndex)throw Error(secret);}});assert.fail('expected synthetic command failure');}
  catch(error){const report=desktopFailure(error);assert.equal(report.buildExecuted,true);assert.equal(report.commandsStarted,failedIndex+1);assert.equal(report.commandsCompleted,failedIndex);assert.equal(report.phase,failedIndex===0?'remote-prepare':'tauri-build');assert.equal(report.code,'DESKTOP_BUILD_FAILED');assert.equal(JSON.stringify(report).includes(secret),false);}
 }
 try{await buildDesktop({repo,environment:official});assert.fail('expected private refusal');}catch(error){const report=desktopFailure(error);assert.equal(report.buildExecuted,false);assert.equal(report.commandsStarted,0);assert.equal(report.phase,'prebuild');}
});
test('actual desktop CLI config override refuses before remote preparation and does not print its value',async t=>{
 const cli=await cliFixture(t);
 const value='synthetic-config-value-must-not-be-logged',result=spawnSync(process.execPath,[cli,'--plan','--config='+value],{encoding:'utf8',env:{...process.env,APPLE_SIGNING_IDENTITY:'-'},timeout:10000});
 assert.equal(result.status,1);const report=JSON.parse(result.stderr);assert.equal(report.code,'DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED');assert.equal(report.commandsStarted,0);assert.equal(result.stderr.includes(value),false);assert.equal(result.stdout,'');
});
test('Cargo config after the literal separator remains local and forwards unchanged',async t=>{
 const repo=await fixture(t),args=['--debug','--','--config','net.offline=true'],commands=[];
 await buildDesktop({repo,environment:{},args},{runCommand:async command=>commands.push(command)});
 assert.equal(commands.length,2);assert.deepEqual(commands[1].args.slice(4),args);
});

test('actual plan with forwarded Cargo configuration does not print argument values',()=>{
 const secret='synthetic-forwarded-value-must-not-be-logged',args=['--','--config','env.EXHIBITOS_SYNTHETIC='+secret];
 const result=spawnSync(process.execPath,[resolve('scripts/build-desktop.mjs'),'--plan',...args],{encoding:'utf8',env:{PATH:process.env.PATH,APPLE_SIGNING_IDENTITY:'-'},timeout:10000});
 assert.equal(result.status,0);assert.equal(result.stderr,'');assert.equal(result.stdout.includes(secret),false);
 const plan=JSON.parse(result.stdout);assert.equal(plan.commands,undefined);assert.deepEqual(plan.stages,['remote-prepare','tauri-build']);assert.equal(plan.forwardedArgumentCount,args.length);assert.equal(plan.buildExecuted,false);
});
test('supported verbose/debug short clusters classify config before preparation without consuming value options',async t=>{
 const repo=await fixture(t);
 for(const args of [['-vcsynthetic.json'],['-dcsynthetic.json'],['-vvcsynthetic.json'],['-vdc','synthetic.json']]){
  let calls=0;await assert.rejects(buildDesktop({repo,environment:{},args},{runCommand:()=>calls++}),/DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED/);assert.equal(calls,0);
 }
 for(const args of [['-rcsynthetic'],['-tcsynthetic'],['-fcsynthetic'],['-bcsynthetic'],['--','-vcsynthetic.json']]){
  const commands=[];await buildDesktop({repo,environment:{},args},{runCommand:async command=>commands.push(command)});
  assert.equal(commands.length,2);assert.deepEqual(commands[1].args.slice(4),args);
 }
});

test('fixed notice-qualified manifest admits read-only official intent without claiming signed release',async t=>{
 const qualified={format:manifest.format,version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:QUALIFIED_REDISTRIBUTION,files:DISTRIBUTION_FILES};
 const repo=await fixture(t,qualified),commands=[];
 const result=await buildDesktop({repo,environment:official,planOnly:true},{runCommand:async command=>commands.push(command)});
 assert.equal(commands.length,0);assert.equal(result.buildExecuted,false);assert.equal(result.productionSigned,false);assert.equal(result.notarized,false);
 for(const options of [{environment:{...official,TAURI_CONFIG:'synthetic'}},{environment:official,args:['--config=synthetic']}]){let calls=0;await assert.rejects(buildDesktop({repo,...options},{runCommand:()=>calls++}),/DESKTOP_RELEASE_CONFIG_UNRECOGNIZED/);assert.equal(calls,0);}
 for(const change of [v=>delete v.files['notices/LICENSE.bdb.txt'],v=>v.files['notices/LICENSE.zlib-ng.txt'].sha256='0'.repeat(64),v=>v.files['metadata/PYTHON.json'].bytes++,v=>v.source.sha256='0'.repeat(64),v=>v.extra=true,v=>v.redistribution='public']){
  const invalid=structuredClone(qualified);change(invalid);await writeFile(join(repo,'src-tauri/update-runtime/generated/manifest.json'),JSON.stringify(invalid));
  let calls=0;await assert.rejects(buildDesktop({repo,environment:official},{runCommand:()=>calls++}),/DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED/);assert.equal(calls,0);
 }
});

test('qualified runtime refuses executable official intent without a reviewed plan before any command',async t=>{
 const qualified={format:manifest.format,version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:QUALIFIED_REDISTRIBUTION,files:DISTRIBUTION_FILES};
 const repo=await fixture(t,qualified);let calls=0;
 for(const environment of [official,{APPLE_PASSWORD:'synthetic-unused'}])for(const executeOfficial of [false,true]){
  await assert.rejects(buildDesktop({repo,environment,executeOfficial},{runCommand:()=>calls++}),/DESKTOP_OFFICIAL_EXECUTION_REQUIRED/);assert.equal(calls,0);
 }
 await writeFile(join(repo,'src-tauri/tauri.conf.json'),JSON.stringify({bundle:{macOS:{signingIdentity:official.APPLE_SIGNING_IDENTITY}}}));
 for(const executeOfficial of [false,true]){
  await assert.rejects(buildDesktop({repo,environment:{},executeOfficial},{runCommand:()=>calls++}),/DESKTOP_OFFICIAL_EXECUTION_REQUIRED/);assert.equal(calls,0);
 }
});

test('official runtime signing is explicitly admitted between remote preparation and native compile, then binds output',async t=>{
 const qualified={format:manifest.format,version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:QUALIFIED_REDISTRIBUTION,files:DISTRIBUTION_FILES};
 const repo=await fixture(t,qualified),events=[];
 const p={version:1,sourceCommit:'a'.repeat(40),sourceTree:'b'.repeat(40),identitySha1:'C'.repeat(40),teamId:'123456789A',notaryProfile:'synthetic-reference',workDir:repo+'-official-output',appPath:repo+'-app/ExhibitOS Manager.app'};
 const hooks={runCommand:async(command,options)=>{events.push(command.args.includes('remote:prepare')?'remote':'compile');assert.equal(options.env.APPLE_SIGNING_IDENTITY,'-');assert.equal(options.env.APPLE_PASSWORD,undefined);},prepareOfficial:async()=>events.push('nested-sign-and-finalize'),recordOfficial:async()=>events.push('bind-built-output')};
 await assert.rejects(buildDesktop({repo,environment:official,officialPlan:p},hooks),/DESKTOP_OFFICIAL_EXECUTION_REQUIRED/);assert.deepEqual(events,[]);
 await buildDesktop({repo,environment:{...official,APPLE_PASSWORD:'synthetic-never-forwarded'},officialPlan:p,executeOfficial:true},hooks);
 assert.deepEqual(events,['remote','nested-sign-and-finalize','compile','bind-built-output']);events.length=0;
 await assert.rejects(buildDesktop({repo,environment:official,officialPlan:p,executeOfficial:true},{...hooks,prepareOfficial:async()=>{throw Error('synthetic sign failure');}}),/synthetic sign failure/);assert.deepEqual(events,['remote']);
});

test('actual desktop CLI with an explicit qualified plan exits without circular module evaluation or commands',async t=>{
 const cli=await cliFixture(t),repo=resolve(cli,'../..');
 const qualified={format:manifest.format,version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:QUALIFIED_REDISTRIBUTION,files:DISTRIBUTION_FILES};
 await writeFile(join(repo,'src-tauri/update-runtime/generated/manifest.json'),JSON.stringify(qualified));
 const planPath=join(repo,'official-plan.json');
 await writeFile(planPath,JSON.stringify({version:1,sourceCommit:'a'.repeat(40),sourceTree:'b'.repeat(40),identitySha1:'C'.repeat(40),teamId:'123456789A',notaryProfile:'synthetic-unused',workDir:repo+'-official-output',appPath:repo+'-target/ExhibitOS Manager.app'}));
 const result=spawnSync(process.execPath,[cli,'--plan','--official-runtime-plan',planPath],{cwd:repo,env:{PATH:'/usr/bin:/bin'},encoding:'utf8',timeout:5000});
 assert.equal(result.error,undefined);assert.equal(result.status,0,result.stderr);assert.equal(result.stderr,'');
 const report=JSON.parse(result.stdout);assert.equal(report.buildExecuted,false);assert.equal(report.commandsStarted,0);assert.equal(report.commandsCompleted,0);assert.equal(report.productionSigned,false);assert.equal(report.notarized,false);
});
