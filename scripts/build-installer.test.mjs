// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,writeFile,rm,symlink,readFile,link,realpath} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {buildInstaller,nativeProfile,planInstaller} from './build-installer.mjs';
async function fixture(t){
 const repo=await mkdtemp(join(await realpath(tmpdir()),'exhi-installer-'));t.after(()=>rm(repo,{recursive:true,force:true}));
 for(const dir of ['src-tauri','node_modules/@tauri-apps/cli'])await mkdir(join(repo,dir),{recursive:true});
 await writeFile(join(repo,'.gitignore'),'node_modules/\ntarget\n');
 await writeFile(join(repo,'node_modules/@tauri-apps/cli/tauri.js'),'// cached synthetic tool; never executed\n');
 await writeFile(join(repo,'package.json'),JSON.stringify({version:'0.1.0'}));
 await writeFile(join(repo,'src-tauri/tauri.conf.json'),JSON.stringify({version:'0.1.0',identifier:'org.exhibitos.manager',productName:'ExhibitOS Manager',bundle:{targets:['app']}}));
 await writeFile(join(repo,'src-tauri/Cargo.toml'),'[package]\nversion = "0.1.0"\n');
 for(const file of ['Cargo.lock','package-lock.json'])await writeFile(join(repo,file),'synthetic lock\n');
 const git=(...args)=>execFileSync('git',args,{cwd:repo,stdio:'ignore'});git('init');git('add','.');git('-c','user.name=Fixture','-c','user.email=fixture@invalid.example','commit','-m','fixture');
 return repo;
}
const options=repo=>({repo,platform:'win32',architecture:'x64'});
async function artifact(repo,name='ExhibitOS Manager_0.1.0_x64-setup.exe',data='synthetic installer'){
 const root=join(repo,'target/release/bundle/nsis');await mkdir(root,{recursive:true});await writeFile(join(root,name),data);return join(root,name);
}
test('native profiles refuse unsupported OS/arch without repository or execution',()=>{
 assert.deepEqual(nativeProfile('win32','x64'),{bundle:'nsis',suffix:'.exe'});assert.equal(nativeProfile('darwin','arm64').bundle,'dmg');
 for(const [p,a] of [['linux','x64'],['win32','arm64'],['darwin','unknown']])assert.throws(()=>nativeProfile(p,a),/INSTALLER_HOST_UNSUPPORTED/);
});
test('Windows fixed Node/Tauri command and genuine file checksum, signing inputs absent',async t=>{
 const repo=await fixture(t);let calls=0;
 const prior=process.env.TAURI_SIGNING_PRIVATE_KEY;process.env.TAURI_SIGNING_PRIVATE_KEY='synthetic-unused';t.after(()=>{if(prior===undefined)delete process.env.TAURI_SIGNING_PRIVATE_KEY;else process.env.TAURI_SIGNING_PRIVATE_KEY=prior;});
 const result=await buildInstaller(options(repo),{runBuild:async(cmd,ctx)=>{calls++;assert.equal(cmd.executable,process.execPath);assert.deepEqual(cmd.args.slice(1),['build','--ci','--bundles','nsis','--','--locked']);assert.equal(ctx.env.TAURI_SIGNING_PRIVATE_KEY,undefined);await artifact(repo);}});
 assert.equal(calls,1);assert.equal(result.productionSigned,false);assert.equal(result.distribution,'unsigned-prototype');assert.equal(result.artifacts[0].bytes,19);assert.equal(result.artifacts[0].sha256,createHash('sha256').update('synthetic installer').digest('hex'));assert.equal(JSON.stringify(result).includes(repo),false);
 const macRepo=await fixture(t);
 const mac=await buildInstaller({repo:macRepo,platform:'darwin',architecture:'arm64'},{runBuild:async()=>{
  const root=join(macRepo,'target/release/bundle/dmg');await mkdir(root,{recursive:true});
  await writeFile(join(root,'ExhibitOS Manager_0.1.0_aarch64.dmg'),'synthetic dmg');
  await writeFile(join(root,'bundle_dmg.sh'),'synthetic packaging script');
  await writeFile(join(root,'icon.icns'),'synthetic icon');
 }});
 assert.equal(mac.artifacts.length,1);assert.deepEqual(mac.packagingAuxiliaries.map(x=>x.name),['bundle_dmg.sh','icon.icns']);
 assert.equal(mac.packagingAuxiliaries[0].sha256,createHash('sha256').update('synthetic packaging script').digest('hex'));
});
test('stale outputs and dirty sources refuse before builder; preserve bytes',async t=>{
 const repo=await fixture(t),path=await artifact(repo);let calls=0;
 await assert.rejects(()=>buildInstaller(options(repo),{runBuild:()=>{calls++;}}),/INSTALLER_OUTPUT_EXISTS/);assert.equal(calls,0);assert.equal(await readFile(path,'utf8'),'synthetic installer');
 await rm(join(repo,'target'),{recursive:true});await writeFile(join(repo,'Cargo.lock'),'changed');await assert.rejects(()=>planInstaller(options(repo)),/INSTALLER_SOURCE_DIRTY/);
});
test('symlink parent refuses before builder even when output absent',async t=>{
 const repo=await fixture(t);await symlink(tmpdir(),join(repo,'target'),'dir');await assert.rejects(()=>planInstaller(options(repo)),/INSTALLER_PATH_UNSAFE/);
});
test('build failure retains partial outputs without receipt',async t=>{
 const repo=await fixture(t);await assert.rejects(()=>buildInstaller(options(repo),{runBuild:async()=>{await artifact(repo);throw Error('synthetic failure');}}),/synthetic failure/);assert.equal(await readFile(join(repo,'target/release/bundle/nsis/ExhibitOS Manager_0.1.0_x64-setup.exe'),'utf8'),'synthetic installer');
});
test('wrong version/path/hardlink and post-build source/tool drift refuse positive receipt',async t=>{
 for(const kind of ['version','version-prefix','name','hardlink','source','tool','unexpected']){
  const repo=await fixture(t);
  await assert.rejects(()=>buildInstaller(options(repo),{runBuild:async()=>{
   const path=await artifact(repo,kind==='version'?'Manager_0.2.0.exe':kind==='version-prefix'?'Manager_0.1.00.exe':kind==='name'?'bad;0.1.0.exe':undefined);
   if(kind==='unexpected')await artifact(repo,'icon.icns');
   if(kind==='hardlink')await link(path,join(repo,'alias'));
   if(kind==='source')await writeFile(join(repo,'Cargo.lock'),'changed');
   if(kind==='tool')await writeFile(join(repo,'node_modules/@tauri-apps/cli/tauri.js'),'changed');
  }}),new RegExp(({version:'INSTALLER_ARTIFACT_INVALID','version-prefix':'INSTALLER_ARTIFACT_INVALID',name:'INSTALLER_ARTIFACT_INVALID',hardlink:'INSTALLER_PATH_UNSAFE',source:'INSTALLER_SOURCE_CHANGED',tool:'INSTALLER_TOOL_CHANGED',unexpected:'INSTALLER_ARTIFACT_INVALID'})[kind]));
 }
 for(const kind of ['auxiliary-symlink','unexpected']){
  const repo=await fixture(t);
  await assert.rejects(()=>buildInstaller({repo,platform:'darwin',architecture:'arm64'},{runBuild:async()=>{
   const root=join(repo,'target/release/bundle/dmg');await mkdir(root,{recursive:true});
   const dmg=join(root,'Manager_0.1.0_arm64.dmg');await writeFile(dmg,'synthetic dmg');
   if(kind==='auxiliary-symlink')await symlink(dmg,join(root,'bundle_dmg.sh'));
   else await writeFile(join(root,'unrelated.txt'),'unexpected');
  }}),kind==='auxiliary-symlink'?/INSTALLER_PATH_UNSAFE/:/INSTALLER_ARTIFACT_INVALID/);
 }
});
