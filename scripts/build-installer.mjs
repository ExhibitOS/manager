// SPDX-License-Identifier: Apache-2.0
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {lstat,open,readdir} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';

const fail=code=>{throw Error(code);};
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const same=(a,b)=>['dev','ino','size','mtimeNs','ctimeNs','mode','nlink'].every(k=>a[k]===b[k]);
const maximumArtifactBytes=2*1024**3;
function source(repo){
  try{
    const opts={cwd:repo,encoding:'utf8',maxBuffer:65536,timeout:10000,windowsHide:true,stdio:['ignore','pipe','pipe']};
    const commit=execFileSync('git',['rev-parse','HEAD'],opts).trim(),tree=execFileSync('git',['rev-parse','HEAD^{tree}'],opts).trim();
    if(!/^[a-f0-9]{40}$/.test(commit)||!/^[a-f0-9]{40}$/.test(tree))fail('INSTALLER_SOURCE_INVALID');
    if(execFileSync('git',['status','--porcelain=v1','--untracked-files=all'],opts).trim())fail('INSTALLER_SOURCE_DIRTY');
    return{commit,tree};
  }catch(e){if(e.message==='INSTALLER_SOURCE_DIRTY')throw e;fail('INSTALLER_SOURCE_INVALID');}
}
function unchangedSource(repo,expected){
  try { if(JSON.stringify(source(repo))!==JSON.stringify(expected))fail('INSTALLER_SOURCE_CHANGED'); }
  catch { fail('INSTALLER_SOURCE_CHANGED'); }
}
async function named(path,kind){
  const stat=await lstat(path,{bigint:true});
  if(stat.isSymbolicLink()||(kind==='directory'?!stat.isDirectory():!stat.isFile()||stat.nlink!==1n))fail('INSTALLER_PATH_UNSAFE');
  return stat;
}
async function parentsAllowMissing(path){
  let current=resolve(path);
  for(;;){try{await named(current,'directory');}catch(e){if(e.code!=='ENOENT')throw e;}const next=dirname(current);if(next===current)return;current=next;}
}
async function parents(path){
  let current=resolve(path);
  for(;;){await named(current,'directory');const next=dirname(current);if(next===current)return;current=next;}
}
async function bytes(path,limit){
  const before=await named(path,'file');if(before.size>BigInt(limit))fail('INSTALLER_FILE_TOO_LARGE');
  const handle=await open(path,'r');
  try{
    if(!same(before,await handle.stat({bigint:true})))fail('INSTALLER_FILE_CHANGED');
    const content=await handle.readFile();
    if(content.length>limit||BigInt(content.length)!==before.size||!same(before,await handle.stat({bigint:true}))||!same(before,await named(path,'file')))fail('INSTALLER_FILE_CHANGED');
    return content;
  }finally{await handle.close();}
}
export function nativeProfile(platform,architecture){
  if(platform==='win32'&&architecture==='x64')return{bundle:'nsis',suffix:'.exe'};
  if(platform==='darwin'&&['x64','arm64'].includes(architecture))return{bundle:'dmg',suffix:'.dmg'};
  fail('INSTALLER_HOST_UNSUPPORTED');
}
export async function planInstaller({repo,platform=process.platform,architecture=process.arch}){
  const profile=nativeProfile(platform,architecture);repo=resolve(repo);await parents(repo);
  const before=source(repo);
  const paths=['package.json','src-tauri/tauri.conf.json','src-tauri/Cargo.toml','Cargo.lock','package-lock.json'];
  const inputs=await Promise.all(paths.map(p=>bytes(join(repo,p),16*1024**2)));
  let pkg,config;
  try{pkg=JSON.parse(inputs[0]);config=JSON.parse(inputs[1]);}catch{fail('INSTALLER_CONFIG_INVALID');}
  const version=pkg.version;
  if(!/^\d+\.\d+\.\d+$/.test(version)||config.version!==version||config.identifier!=='org.exhibitos.manager'||config.productName!=='ExhibitOS Manager'||!new RegExp('^version\\s*=\\s*"'+version.replaceAll('.','\\.')+'"\\s*$','m').test(inputs[2].toString('utf8')))fail('INSTALLER_VERSION_INVALID');
  // This entry point deliberately has no signing, updater or configuration override.
  if(config.bundle?.macOS?.signingIdentity||config.bundle?.windows?.certificateThumbprint||config.bundle?.windows?.signCommand||config.bundle?.createUpdaterArtifacts)fail('INSTALLER_SIGNING_CONFIG_UNSUPPORTED');
  const output=join(repo,'target','release','bundle',profile.bundle);
  await parentsAllowMissing(output);
  try{if((await readdir(output)).length)fail('INSTALLER_OUTPUT_EXISTS');}catch(e){if(e.code!=='ENOENT')throw e;}
  const cli=join(repo,'node_modules','@tauri-apps','cli','tauri.js');await parents(dirname(cli));
  const cliSha256=hash(await bytes(cli,1024*1024));
  unchangedSource(repo,before);
  return{repo,platform,architecture,version,source:before,profile,output,inputHashes:paths.map((name,i)=>({name,sha256:hash(inputs[i])})),cliSha256,command:{executable:process.execPath,args:[cli,'build','--ci','--bundles',profile.bundle,'--','--locked']}};
}
async function inventory(plan){
  await parents(plan.output);const directory=await named(plan.output,'directory'),names=(await readdir(plan.output)).sort();
  if(!names.length||names.length>8)fail('INSTALLER_ARTIFACT_COUNT_INVALID');
  const handles=[],artifacts=[],packagingAuxiliaries=[];let total=0;
  const allowedAuxiliaries=plan.profile.bundle==='dmg'?new Set(['bundle_dmg.sh','icon.icns']):new Set();
  try{
    for(const name of names){
      const auxiliary=allowedAuxiliaries.has(name);
      if(!auxiliary && (!/^[a-zA-Z0-9][a-zA-Z0-9._ -]{0,180}$/.test(name)||!name.endsWith(plan.profile.suffix)||!new RegExp('(?:^|[_ -])'+plan.version.replaceAll('.','\\.')+'(?:[_ -]|\\.)').test(name)))fail('INSTALLER_ARTIFACT_INVALID');
      const path=join(plan.output,name),before=await named(path,'file');
      if(before.size<=0n||before.size>BigInt(maximumArtifactBytes))fail('INSTALLER_FILE_TOO_LARGE');
      total+=Number(before.size);if(total>4*1024**3)fail('INSTALLER_FILE_TOO_LARGE');
      const handle=await open(path,'r');handles.push({path,handle,before});
      if(!same(before,await handle.stat({bigint:true})))fail('INSTALLER_FILE_CHANGED');
      const digest=createHash('sha256'),buffer=Buffer.alloc(65536);let count=0;
      for(;;){const r=await handle.read(buffer,0,buffer.length,count);if(!r.bytesRead)break;count+=r.bytesRead;if(BigInt(count)>before.size)fail('INSTALLER_FILE_CHANGED');digest.update(buffer.subarray(0,r.bytesRead));}
      if(BigInt(count)!==before.size)fail('INSTALLER_FILE_CHANGED');
      (auxiliary?packagingAuxiliaries:artifacts).push({name,bytes:count,sha256:digest.digest('hex')});
    }
    for(const {path,handle,before} of handles)if(!same(before,await handle.stat({bigint:true}))||!same(before,await named(path,'file')))fail('INSTALLER_FILE_CHANGED');
    if(!same(directory,await named(plan.output,'directory'))||JSON.stringify(names)!==JSON.stringify((await readdir(plan.output)).sort()))fail('INSTALLER_OUTPUT_CHANGED');
    unchangedSource(plan.repo,plan.source);
    for(const input of plan.inputHashes)if(hash(await bytes(join(plan.repo,input.name),16*1024**2))!==input.sha256)fail('INSTALLER_SOURCE_CHANGED');
    if(!artifacts.length)fail('INSTALLER_ARTIFACT_COUNT_INVALID');
    return {artifacts,packagingAuxiliaries};
  }finally{await Promise.allSettled(handles.map(({handle})=>handle.close()));}
}
export async function buildInstaller(options,{runBuild}={}){
  const plan=await planInstaller(options);
  const env={...process.env};for(const key of Object.keys(env))if(/^(APPLE_|TAURI_|WINDOWS_CERTIFICATE|CSC_|WIN_CSC_)/.test(key))delete env[key];
  if(runBuild)await runBuild(plan.command,{cwd:plan.repo,env});
  else{
    try{execFileSync(plan.command.executable,plan.command.args,{cwd:plan.repo,env,stdio:['ignore',2,2],timeout:30*60*1000,windowsHide:true,shell:false});}
    catch{fail('INSTALLER_BUILD_FAILED');}
  }
  if(hash(await bytes(plan.command.args[0],1024*1024))!==plan.cliSha256)fail('INSTALLER_TOOL_CHANGED');
  const result=await inventory(plan);
  return{format:1,observedAt:new Date().toISOString(),platform:plan.platform,architecture:plan.architecture,version:plan.version,source:plan.source,inputHashes:plan.inputHashes,bundle:plan.profile.bundle,packagingToolSha256:plan.cliSha256,artifacts:result.artifacts,packagingAuxiliaries:result.packagingAuxiliaries,distribution:'unsigned-prototype',signingRequested:false,signatureVerification:'not_run',productionSigned:false,scope:'Source-fenced local packaging and byte checksums only; not signing, installation, uninstall, update, offline Runtime or recovery qualification.'};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  try{
    const args=process.argv.slice(2);if(args.length!==1||!['--plan','--build'].includes(args[0]))fail('INSTALLER_USAGE');
    const options={repo:resolve(fileURLToPath(new URL('../',import.meta.url)))};
    if(args[0]==='--plan'){const p=await planInstaller(options);console.log(JSON.stringify({format:1,platform:p.platform,architecture:p.architecture,version:p.version,source:p.source,bundle:p.profile.bundle,command:p.command,inputHashes:p.inputHashes,distribution:'unsigned-prototype',buildExecuted:false},null,2));}
    else console.log(JSON.stringify(await buildInstaller(options),null,2));
  }catch(e){console.error(/^INSTALLER_[A-Z_]+$/.test(e.message)?e.message:'INSTALLER_FAILED');process.exitCode=1;}
}
