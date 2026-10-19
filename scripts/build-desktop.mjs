// SPDX-License-Identifier: Apache-2.0
import {spawnSync} from 'node:child_process';
import {constants} from 'node:fs';
import {lstat,open,readFile,realpath} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';

const fail=code=>{throw Error(code);};
const same=(a,b)=>['dev','ino','size','mtimeNs','ctimeNs','mode','nlink'].every(k=>a[k]===b[k]);
const manifestLimit=2*1024**2;
const hasValue=value=>typeof value==='string'&&value.trim().length>0;
// Named Apple Development and ad-hoc identities retain the local build route.
// An opaque identity cannot be classified without Keychain access; use the
// existing dedicated development signer for a selected certificate SHA1.
const releaseIdentity=value=>hasValue(value)&&value.trim()!=='-'&&!/^Apple Development: .+/.test(value.trim());
export function officialIntent(environment,config,args=[]){
 const separator=args.indexOf('--'),tauriArgs=separator<0?args:args.slice(0,separator);
 return releaseIdentity(environment.APPLE_SIGNING_IDENTITY)||releaseIdentity(config.bundle?.macOS?.signingIdentity)||
  ['APPLE_CERTIFICATE','APPLE_CERTIFICATE_PASSWORD','APPLE_ID','APPLE_PASSWORD','APPLE_TEAM_ID','APPLE_API_KEY','APPLE_API_ISSUER','APPLE_API_KEY_PATH','TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD'].some(key=>hasValue(environment[key]))||
  (config.bundle?.createUpdaterArtifacts!==undefined&&config.bundle.createUpdaterArtifacts!==false)||hasValue(environment.TAURI_CONFIG)||tauriArgs.some(arg=>arg==='--config'||arg.startsWith('--config=')||arg==='-c'||(arg.startsWith('-c')&&arg.length>2));
}
async function guardedManifest(repo){
 const folder=join(repo,'src-tauri/update-runtime/generated'),file=join(folder,'manifest.json');
 try{
  if(await realpath(repo)!==repo)fail('DESKTOP_RELEASE_RUNTIME_UNSAFE');
  let current=folder;
  for(;;){const info=await lstat(current);if(!info.isDirectory()||info.isSymbolicLink())fail('DESKTOP_RELEASE_RUNTIME_UNSAFE');if(current===repo)break;current=dirname(current);}
  const before=await lstat(file,{bigint:true});
  if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1n)fail('DESKTOP_RELEASE_RUNTIME_UNSAFE');
  if(before.size<=0n||before.size>BigInt(manifestLimit))fail('DESKTOP_RELEASE_RUNTIME_TOO_LARGE');
  const handle=await open(file,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
  let data;
  try{
   if(!same(before,await handle.stat({bigint:true})))fail('DESKTOP_RELEASE_RUNTIME_CHANGED');
   data=Buffer.alloc(Number(before.size)+1);let count=0;
   while(count<data.length){const read=await handle.read(data,count,data.length-count,null);if(!read.bytesRead)break;count+=read.bytesRead;}
   if(BigInt(count)!==before.size||!same(before,await handle.stat({bigint:true}))||!same(before,await lstat(file,{bigint:true})))fail('DESKTOP_RELEASE_RUNTIME_CHANGED');
   data=data.subarray(0,count);
  }finally{await handle.close();}
  let manifest;try{manifest=JSON.parse(data);}catch{fail('DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED');}
  if(manifest?.format!=='exhibitos-manager-update-runtime-v1'||manifest.version!==1||manifest.redistribution!=='private-validation-only')fail('DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED');
  // This is the only existing producer/native-consumer contract. Relabelling
  // it "public" cannot qualify rights or create an official release runtime.
  fail('DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED');
 }catch(error){if(error.code==='ENOENT')fail('DESKTOP_RELEASE_RUNTIME_MISSING');if(/^DESKTOP_[A-Z_]+$/.test(error.message))throw error;fail('DESKTOP_RELEASE_RUNTIME_UNSAFE');}
}
export function desktopFailure(error){
 const progress=error?.desktopProgress??{phase:'prebuild',commandsStarted:0,commandsCompleted:0};
 return{code:/^DESKTOP_[A-Z_]+$/.test(error?.message)?error.message:'DESKTOP_BUILD_FAILED',...progress,buildExecuted:progress.commandsStarted>0,officialReleaseQualified:false};
}
export async function buildDesktop({repo,environment=process.env,planOnly=false,args=[]},{runCommand}={}){
 const progress={phase:'prebuild',commandsStarted:0,commandsCompleted:0};
 try{
  repo=resolve(repo);
  let config;try{config=JSON.parse(await readFile(join(repo,'src-tauri/tauri.conf.json'),'utf8'));}catch{fail('DESKTOP_CONFIG_INVALID');}
  if(officialIntent(environment,config,args))await guardedManifest(repo);
  const commands=[{executable:process.platform==='win32'?'npm.cmd':'npm',args:['run','remote:prepare'],shell:process.platform==='win32'},{executable:process.execPath,args:[join(repo,'node_modules/@tauri-apps/cli/tauri.js'),'build','--bundles','app',...args],shell:false}];
  if(!planOnly)for(const [index,command] of commands.entries()){
   progress.phase=index===0?'remote-prepare':'tauri-build';
   if(runCommand){progress.commandsStarted++;await runCommand(command,{cwd:repo,env:environment});}
   else{
    const result=spawnSync(command.executable,command.args,{cwd:repo,env:environment,stdio:'inherit',windowsHide:true,shell:command.shell});
    if(result.pid>0)progress.commandsStarted++;
    if(result.error)fail('DESKTOP_COMMAND_START_FAILED');
    if(result.status!==0)fail('DESKTOP_COMMAND_FAILED');
   }
   progress.commandsCompleted++;
  }
  return{format:1,distribution:'local-development',...progress,buildExecuted:progress.commandsStarted>0,productionSigned:false,notarized:false,officialReleaseQualified:false,...(planOnly?{stages:['remote-prepare','tauri-build'],forwardedArgumentCount:args.length}:{} )};
 }catch(error){const failure=error instanceof Error?error:Error('DESKTOP_BUILD_FAILED');failure.desktopProgress={...progress};throw failure;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const args=process.argv.slice(2),planOnly=args[0]==='--plan';if(planOnly)args.shift();
  const repo=resolve(fileURLToPath(new URL('../',import.meta.url)));
  console.log(JSON.stringify(await buildDesktop({repo,planOnly,args}),null,2));
 }catch(error){console.error(JSON.stringify(desktopFailure(error)));process.exitCode=1;}
}
