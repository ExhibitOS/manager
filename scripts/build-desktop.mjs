// SPDX-License-Identifier: Apache-2.0
import {spawnSync} from 'node:child_process';
import {readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {assertOfficialRuntime} from './official-runtime-guard.mjs';
export {assertOfficialRuntime} from './official-runtime-guard.mjs';
import {fileURLToPath,pathToFileURL} from 'node:url';

const fail=code=>{throw Error(code);};
const hasValue=value=>typeof value==='string'&&value.trim().length>0;
// Named Apple Development and ad-hoc identities retain the local build route.
// An opaque identity cannot be classified without Keychain access; use the
// existing dedicated development signer for a selected certificate SHA1.
const releaseIdentity=value=>hasValue(value)&&value.trim()!=='-'&&!/^Apple Development: .+/.test(value.trim());
export function officialIntent(environment,config,args=[]){
 const separator=args.indexOf('--'),tauriArgs=separator<0?args:args.slice(0,separator);
 return releaseIdentity(environment.APPLE_SIGNING_IDENTITY)||releaseIdentity(config.bundle?.macOS?.signingIdentity)||
  ['APPLE_CERTIFICATE','APPLE_CERTIFICATE_PASSWORD','APPLE_ID','APPLE_PASSWORD','APPLE_TEAM_ID','APPLE_API_KEY','APPLE_API_ISSUER','APPLE_API_KEY_PATH','TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD'].some(key=>hasValue(environment[key]))||
  (config.bundle?.createUpdaterArtifacts!==undefined&&config.bundle.createUpdaterArtifacts!==false)||hasValue(environment.TAURI_CONFIG)||tauriArgs.some(arg=>arg==='--config'||arg.startsWith('--config=')||/^-[vd]*c/.test(arg));
}
export function desktopFailure(error){
 const progress=error?.desktopProgress??{phase:'prebuild',commandsStarted:0,commandsCompleted:0};
 return{code:/^DESKTOP_[A-Z_]+$/.test(error?.message)?error.message:'DESKTOP_BUILD_FAILED',...progress,buildExecuted:progress.commandsStarted>0,officialReleaseQualified:false};
}
export async function buildDesktop({repo,environment=process.env,planOnly=false,args=[],officialPlan=null,executeOfficial=false},{runCommand,prepareOfficial,recordOfficial}={}){
 const progress={phase:'prebuild',commandsStarted:0,commandsCompleted:0};
 try{
  repo=resolve(repo);
  let config;try{config=JSON.parse(await readFile(join(repo,'src-tauri/tauri.conf.json'),'utf8'));}catch{fail('DESKTOP_CONFIG_INVALID');}
  const officialRequested=Boolean(officialPlan)||officialIntent(environment,config,args);
  if(officialRequested){
   await assertOfficialRuntime(repo);
   const separator=args.indexOf('--'),tauriArgs=separator<0?args:args.slice(0,separator);
   // An arbitrary config override can replace the qualified resource mapping.
   // Qualification of the source config does not qualify an unread overlay.
   if(hasValue(environment.TAURI_CONFIG)||tauriArgs.some(arg=>arg==='--config'||arg.startsWith('--config=')||/^-[vd]*c/.test(arg)))fail('DESKTOP_RELEASE_CONFIG_UNRECOGNIZED');
  }
  if(officialPlan){const {validateOfficialPlan}=await import('./official-mac-distribution.mjs');validateOfficialPlan(officialPlan,repo);}
  if(officialRequested&&!planOnly&&(!officialPlan||!executeOfficial))fail('DESKTOP_OFFICIAL_EXECUTION_REQUIRED');
  if(args.some(a=>a==='--official-runtime-plan'||a==='--execute-official-distribution'))fail('DESKTOP_OFFICIAL_ARGUMENTS_INVALID');
  const buildEnvironment={...environment};
  if(officialPlan){if(args.some(a=>a==='--target'||a.startsWith('--target=')||a==='--bundles'||a.startsWith('--bundles=')))fail('DESKTOP_OFFICIAL_ARGUMENTS_INVALID');for(const key of Object.keys(buildEnvironment))if(/^(APPLE_|CSC_|TAURI_SIGNING_)/.test(key))delete buildEnvironment[key];buildEnvironment.APPLE_SIGNING_IDENTITY='-';}
  const commands=[{executable:process.platform==='win32'?'npm.cmd':'npm',args:['run','remote:prepare'],shell:process.platform==='win32'},{executable:process.execPath,args:[join(repo,'node_modules/@tauri-apps/cli/tauri.js'),'build','--bundles','app',...args],shell:false}];
  if(!planOnly)for(const [index,command] of commands.entries()){
   progress.phase=index===0?'remote-prepare':'tauri-build';
   if(index===1&&officialPlan){const prepare=prepareOfficial??(await import('./official-mac-distribution.mjs')).prepareOfficialRuntime;await prepare(officialPlan,{repo,execute:true});}
   if(runCommand){progress.commandsStarted++;await runCommand(command,{cwd:repo,env:buildEnvironment});}
   else{
    const result=spawnSync(command.executable,command.args,{cwd:repo,env:buildEnvironment,stdio:'inherit',windowsHide:true,shell:command.shell});
    if(result.pid>0)progress.commandsStarted++;
    if(result.error)fail('DESKTOP_COMMAND_START_FAILED');
    if(result.status!==0)fail('DESKTOP_COMMAND_FAILED');
   }
   progress.commandsCompleted++;
   if(index===1&&officialPlan){const record=recordOfficial??(await import('./official-mac-distribution.mjs')).recordOfficialBuild;const target=buildEnvironment.CARGO_TARGET_DIR??join(repo,'target');await record(officialPlan,{repo,expectedAppPath:join(target,args.includes('--debug')?'debug':'release','bundle/macos/ExhibitOS Manager.app')});}
  }
  return{format:1,distribution:'local-development',...progress,buildExecuted:progress.commandsStarted>0,productionSigned:false,notarized:false,officialReleaseQualified:false,...(planOnly?{stages:['remote-prepare','tauri-build'],forwardedArgumentCount:args.length}:{} )};
 }catch(error){const failure=error instanceof Error?error:Error('DESKTOP_BUILD_FAILED');failure.desktopProgress={...progress};throw failure;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const args=process.argv.slice(2),planOnly=args[0]==='--plan';if(planOnly)args.shift();
  const repo=resolve(fileURLToPath(new URL('../',import.meta.url)));let officialPlan=null;
  if(args[0]==='--official-runtime-plan'){
   args.shift();const path=args.shift();if(!path||resolve(path)!==path)fail('DESKTOP_OFFICIAL_PLAN_INVALID');
   officialPlan=JSON.parse(await readFile(path,'utf8'));const {validateOfficialPlan}=await import('./official-mac-distribution.mjs');validateOfficialPlan(officialPlan,repo);
   if(!planOnly&&args.shift()!=='--execute-official-distribution')fail('DESKTOP_OFFICIAL_EXECUTION_REQUIRED');
  }
  console.log(JSON.stringify(await buildDesktop({repo,planOnly,args,officialPlan,executeOfficial:!!officialPlan&&!planOnly}),null,2));
 }catch(error){console.error(JSON.stringify(desktopFailure(error)));process.exitCode=1;}
}
