// SPDX-License-Identifier: Apache-2.0
// Fixed Manager arm64 distribution stages. No credential creation, publication or OS policy changes.
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {lstat,realpath,open,writeFile,mkdir,rename,cp,statfs,readdir,unlink} from 'node:fs/promises';
import {constants} from 'node:fs';
import {assertOfficialRuntime} from './official-runtime-guard.mjs';
import {dirname,join,resolve,isAbsolute} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {matchesDistributionFiles,QUALIFIED_REDISTRIBUTION} from './update-runtime-distribution.mjs';
const fail=code=>{throw Error(code);},sha=b=>createHash('sha256').update(b).digest('hex');
const repoRoot=resolve(fileURLToPath(new URL('../',import.meta.url)));
const exact=(v,keys)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===keys.split(',').sort().join(',');
const absolute=p=>typeof p==='string'&&isAbsolute(p)&&resolve(p)===p&&!/[\r\n\0]/.test(p);
const within=(p,root)=>p===root||p.startsWith(root.endsWith('/')?root:root+'/');
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-(?:[a-f0-9]{4}-){3}[a-f0-9]{12}$/.test(v);
const HEX=/^[a-f0-9]{40}$/;
export function validateOfficialPlan(p,repo=repoRoot){
 if(!exact(p,'version,sourceCommit,sourceTree,identitySha1,teamId,notaryProfile,workDir,appPath')||p.version!==1||!HEX.test(p.sourceCommit)||!HEX.test(p.sourceTree)||!/^[A-F0-9]{40}$/.test(p.identitySha1)||!/^[A-Z0-9]{10}$/.test(p.teamId)||typeof p.notaryProfile!=='string'||!p.notaryProfile.length||p.notaryProfile.length>128||/[\r\n\0]/.test(p.notaryProfile)||!absolute(p.workDir)||!absolute(p.appPath)||within(p.workDir,repo)||within(repo,p.workDir)||within(p.workDir,p.appPath)||within(p.appPath,p.workDir))fail('OFFICIAL_PLAN_INVALID');
 return p;
}
const same=(a,b)=>['dev','ino','mode','nlink','size','mtimeMs','ctimeMs'].every(k=>a[k]===b[k]);
async function regular(path,max=1024**3){
 const s=await lstat(path);if(!s.isFile()||s.isSymbolicLink()||s.nlink!==1||(s.mode&0o022)!==0||s.size>max||await realpath(path)!==path)fail('OFFICIAL_FILE_UNSAFE');
 const fd=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
 try{if(!same(s,await fd.stat()))fail('OFFICIAL_FILE_CHANGED');const buffer=Buffer.alloc(s.size+1);let count=0;while(count<buffer.length){const result=await fd.read(buffer,count,buffer.length-count,null);if(!result.bytesRead)break;count+=result.bytesRead;}if(count!==s.size||!same(s,await fd.stat())||!same(s,await lstat(path)))fail('OFFICIAL_FILE_CHANGED');return {bytes:buffer.subarray(0,count),mode:s.mode&0o777};}finally{await fd.close();}
}
async function directory(path){if(await realpath(path)!==path||!(await lstat(path)).isDirectory())fail('OFFICIAL_DIRECTORY_UNSAFE');}
async function json(path){return JSON.parse((await regular(path,4*1024**2)).bytes);}
async function save(path,value,mode=0o600,compact=false){const tmp=path+'.next';await writeFile(tmp,JSON.stringify(value,null,compact?0:2)+'\n',{flag:'wx',mode});await rename(tmp,path);}
// build.rs transports this descriptor through the single-line cargo:rustc-env protocol.
export async function saveRemoteRuntimeManifest(path,value){await save(path,value,0o600,true);}
function command(executable,args){return new Promise((yes,no)=>{const child=spawn(executable,args,{env:{PATH:'/usr/bin:/bin:/usr/sbin:/sbin',DEVELOPER_DIR:'/Applications/Xcode.app/Contents/Developer',...(process.env.HOME?{HOME:process.env.HOME}:{})},stdio:['ignore','pipe','pipe']});let bytes=0,out='',err='';const timer=setTimeout(()=>{child.kill('SIGTERM');no(Error('OFFICIAL_COMMAND_UNCERTAIN'));},900000);for(const [pipe,key]of [[child.stdout,'out'],[child.stderr,'err']])pipe.on('data',data=>{bytes+=data.length;if(bytes>4*1024**2){child.kill('SIGTERM');no(Error('OFFICIAL_OUTPUT_BOUND'));}if(key==='out')out+=data;else err+=data;});child.on('error',()=>{clearTimeout(timer);no(Error('OFFICIAL_COMMAND_FAILED'));});child.on('close',code=>{clearTimeout(timer);if(code!==0)no(Error('OFFICIAL_COMMAND_FAILED'));else yes(args[0]==='notarytool'&&args.includes('--output-format')?out:out+err);});});}
async function gitSource(repo,p,run){for(const [args,expected]of [[['rev-parse','HEAD'],p.sourceCommit],[['rev-parse','HEAD^{tree}'],p.sourceTree],[['status','--porcelain=v1','--untracked-files=all'],'']])if((await run('/usr/bin/git',['-C',repo,...args])).trim()!==expected)fail('OFFICIAL_SOURCE_CHANGED');}
export async function resourceInventory(root){
 const result={};
 async function visit(path){const s=await lstat(path);if(s.isSymbolicLink())fail('OFFICIAL_FILE_UNSAFE');if(s.isDirectory()){for(const name of await readdir(path))await visit(join(path,name));}else{const f=await regular(path);result[path.slice(root.length+1)]={sha256:sha(f.bytes),bytes:f.bytes.length,mode:f.mode};}}
 await directory(root);await visit(root);return Object.fromEntries(Object.entries(result).sort(([a],[b])=>a<b?-1:a>b?1:0));
}
function macho(bytes){return bytes.length>=4&&['cffaedfe','cefaedfe','cafebabe','bebafeca','cafebabf','bfbafeca'].includes(bytes.subarray(0,4).toString('hex'));}
async function stateFor(p){await directory(p.workDir);const s=await json(join(p.workDir,'operation.json'));if(s.planSha256!==sha(Buffer.from(JSON.stringify(p)))||s.sourceCommit!==p.sourceCommit)fail('OFFICIAL_OPERATION_CHANGED');return s;}
async function assertSignature(path,p,node,run){const report=await run('/usr/bin/codesign',['--display','--verbose=4','--entitlements',':-',path]);if(!report.includes('Authority=Developer ID Application:')||!report.includes('TeamIdentifier='+p.teamId)||!report.includes('Timestamp=')||!/flags=.*\bruntime\b/.test(report)||report.includes('com.apple.security.get-task-allow'))fail('OFFICIAL_SIGNATURE_INVALID');const keys=[...report.matchAll(/<key>([^<]+)<\/key>/g)].map(v=>v[1]);if(keys.some(k=>k!=='com.apple.security.cs.allow-jit')||(!node&&keys.length)||node&&(!keys.includes('com.apple.security.cs.allow-jit')||!/<key>com\.apple\.security\.cs\.allow-jit<\/key>\s*<true\s*\/>/.test(report)))fail('OFFICIAL_ENTITLEMENTS_CHANGED');await run('/usr/bin/codesign',['--verify','--strict',path]);}
export async function signNestedRuntime(tasks,p,{repo=repoRoot,run=command,checkpoint=async()=>{}}={}){
 const ordered=[...tasks].sort((a,b)=>Number(a.path.endsWith('/python3.11'))-Number(b.path.endsWith('/python3.11'))||a.path.localeCompare(b.path));
 for(const task of ordered){
  const args=['--force','--sign',p.identitySha1,'--timestamp','--options','runtime',...(task.node?['--entitlements',join(repo,'scripts/macos-node-release.entitlements')]:[]),task.path];
  await run('/usr/bin/codesign',args);await assertSignature(task.path,p,task.node,run);await checkpoint(task.path,sha((await regular(task.path)).bytes));
 }
}
export async function prepareOfficialRuntime(p,{repo=repoRoot,run=command,host=process.platform+'-'+process.arch,execute=false}={}){
 validateOfficialPlan(p,repo);if(!execute)fail('OFFICIAL_EXPLICIT_EXECUTION_REQUIRED');if(host!=='darwin-arm64')fail('OFFICIAL_HOST_UNSUPPORTED');await directory(dirname(p.workDir));await gitSource(repo,p,run);await assertOfficialRuntime(repo);
 const space=await statfs(dirname(p.workDir),{bigint:true});if(space.bavail*space.bsize<8n*1024n**3n)fail('OFFICIAL_SPACE_REQUIRED');
 await mkdir(p.workDir,{mode:0o700});let s={version:1,planSha256:sha(Buffer.from(JSON.stringify(p))),sourceCommit:p.sourceCommit,state:'preparing',signedFiles:[],commands:[],originalsRetained:true};const state=join(p.workDir,'operation.json');await save(state,s);
 try{
  const update=join(repo,'src-tauri/update-runtime/generated'),remote=join(repo,'src-tauri/remote-runtime/generated');const u=await json(join(update,'manifest.json')),r=await json(join(remote,'manifest.json'));
  if(u.redistribution!==QUALIFIED_REDISTRIBUTION||!matchesDistributionFiles(u.files)||r.nodeVersion!=='24.21.0'||r.platform!=='darwin-arm64')fail('OFFICIAL_RUNTIME_UNQUALIFIED');
  const tasks=[];
  for(const [name,entry]of Object.entries(u.files)){const path=join(update,name);if(!within(path,update)||resolve(path)!==path)fail('OFFICIAL_RUNTIME_UNSAFE');const f=await regular(path);if(sha(f.bytes)!==entry.sha256||f.bytes.length!==entry.bytes||f.mode!==entry.mode)fail('OFFICIAL_RUNTIME_CHANGED');if(macho(f.bytes))tasks.push({path,node:false});}
  const node=join(remote,'node'),nf=await regular(node);if(sha(nf.bytes)!==r.nodeSha256||!macho(nf.bytes))fail('OFFICIAL_RUNTIME_CHANGED');tasks.push({path:node,node:true});
  // Libraries before their interpreter; only Node gets the narrowly scoped JIT exception.
  tasks.sort((a,b)=>Number(a.path.endsWith('/python3.11'))-Number(b.path.endsWith('/python3.11'))||a.path.localeCompare(b.path));
  await mkdir(join(p.workDir,'originals'),{mode:0o700});
  for(const path of [join(update,'manifest.json'),join(remote,'manifest.json'),...tasks.map(t=>t.path)]){const index=s.signedFiles.length,original=join(p.workDir,'originals',String(index)),before=await regular(path);await writeFile(original,before.bytes,{flag:'wx',mode:0o600});if(sha((await regular(path)).bytes)!==sha(before.bytes))fail('OFFICIAL_RUNTIME_CHANGED');s.signedFiles.push({path,original,before:sha(before.bytes),beforeMode:before.mode});await save(state,s);}
  await signNestedRuntime(tasks,p,{repo,run,checkpoint:async(path,after)=>{s.signedFiles.find(v=>v.path===path).after=after;await save(state,s);}});
  for(const [name,entry]of Object.entries(u.files)){const f=await regular(join(update,name));entry.sha256=sha(f.bytes);entry.bytes=f.bytes.length;entry.mode=f.mode;}
  if(!matchesDistributionFiles(u.files))fail('OFFICIAL_NOTICE_CHANGED');r.nodeSha256=sha((await regular(node)).bytes);await save(join(update,'manifest.json'),u,0o644);await saveRemoteRuntimeManifest(join(remote,'manifest.json'),r);
  s.state='runtime-prepared';s.manifests={update:sha((await regular(join(update,'manifest.json'))).bytes),remote:sha((await regular(join(remote,'manifest.json'))).bytes)};s.resourceFiles={update:await resourceInventory(update),remote:await resourceInventory(remote)};await save(state,s);return {state:s.state,operation:state};
 }catch(error){s.state='preparation-failed';s.code=/^OFFICIAL_/.test(error.message)?error.message:'OFFICIAL_PREPARATION_FAILED';await save(state,s);throw Error(s.code);}
}
export async function recordOfficialBuild(p,{repo=repoRoot,run=command,expectedAppPath}={}){
 validateOfficialPlan(p,repo);if(p.appPath!==expectedAppPath)fail('OFFICIAL_BUILD_OUTPUT_CHANGED');await gitSource(repo,p,run);const s=await stateFor(p);if(s.state!=='runtime-prepared')fail('OFFICIAL_STATE_REFUSED');
 for(const [name,expected]of [['update-runtime',s.resourceFiles.update],['remote-runtime',s.resourceFiles.remote]])if(JSON.stringify(await resourceInventory(join(p.appPath,'Contents/Resources',name,'generated')))!==JSON.stringify(expected))fail('OFFICIAL_COMPILED_RESOURCES_CHANGED');
 const executable=(await regular(join(p.appPath,'Contents/MacOS/exhibitos-manager-desktop'))).bytes;if(executable.length<16||executable.readUInt32LE(0)!==0xfeedfacf||executable.readUInt32LE(4)!==0x0100000c||executable.readUInt32LE(12)!==2)fail('OFFICIAL_BUILD_ARCH_UNSUPPORTED');
 s.buildAppInventorySha256=sha(Buffer.from(JSON.stringify(await resourceInventory(p.appPath))));s.state='build-bound';await save(join(p.workDir,'operation.json'),s);return {state:s.state};
}
export async function finishOfficialApp(p,{repo=repoRoot,run=command,execute=false}={}){
 validateOfficialPlan(p,repo);if(!execute)fail('OFFICIAL_EXPLICIT_EXECUTION_REQUIRED');await gitSource(repo,p,run);let s=await stateFor(p);if(s.state!=='build-bound')fail('OFFICIAL_STATE_REFUSED');
 const before=await resourceInventory(p.appPath);if(sha(Buffer.from(JSON.stringify(before)))!==s.buildAppInventorySha256)fail('OFFICIAL_BUILD_OUTPUT_CHANGED');
 const space=await statfs(p.workDir,{bigint:true}),sourceBytes=Object.values(before).reduce((n,e)=>n+BigInt(e.bytes),0n);if(space.bavail*space.bsize<sourceBytes*3n+6n*1024n**3n)fail('OFFICIAL_SPACE_REQUIRED');
 const destination=join(p.workDir,'signed-app','ExhibitOS Manager.app');await mkdir(dirname(destination),{mode:0o700});await cp(p.appPath,destination,{recursive:true,errorOnExist:true,force:false});
 try{
  const resources=join(destination,'Contents/Resources');for(const [name,expected]of [['update',s.resourceFiles.update],['remote',s.resourceFiles.remote]]){const dir=name==='update'?'update-runtime':'remote-runtime';if(JSON.stringify(await resourceInventory(join(resources,dir,'generated')))!==JSON.stringify(expected))fail('OFFICIAL_COMPILED_RESOURCES_CHANGED');}
  const binary=join(destination,'Contents/MacOS/exhibitos-manager-desktop'),data=(await regular(binary)).bytes;for(const name of ['update-runtime','remote-runtime']){const manifest=(await regular(join(resources,name,'generated/manifest.json'))).bytes;if(!data.includes(name==='remote-runtime'?Buffer.from(manifest.toString().trim()):manifest))fail('OFFICIAL_COMPILED_MANIFEST_MISSING');}
  const resourceBefore=await resourceInventory(resources);await run('/usr/bin/codesign',['--force','--sign',p.identitySha1,'--timestamp','--options','runtime','--identifier','org.exhibitos.manager',destination]);await assertSignature(destination,p,false,run);await run('/usr/bin/codesign',['--verify','--deep','--strict',destination]);if(JSON.stringify(await resourceInventory(resources))!==JSON.stringify(resourceBefore)||JSON.stringify(await resourceInventory(p.appPath))!==JSON.stringify(before))fail('OFFICIAL_ORIGINAL_OR_RESOURCES_CHANGED');
  const dmg=join(p.workDir,'ExhibitOS-Manager.dmg');await run('/usr/bin/hdiutil',['create','-srcfolder',dirname(destination),'-volname','ExhibitOS Manager','-fs','HFS+','-format','UDZO',dmg]);await run('/usr/bin/codesign',['--force','--sign',p.identitySha1,'--timestamp',dmg]);await run('/usr/bin/codesign',['--verify','--strict',dmg]);await run('/usr/bin/hdiutil',['verify',dmg]);
  const mount=join(p.workDir,'mounted-image');await mkdir(mount,{mode:0o700});let attached=true;
  try{await run('/usr/bin/hdiutil',['attach','-readonly','-nobrowse','-noautoopen','-mountpoint',mount,dmg]);if(JSON.stringify(await resourceInventory(join(mount,'ExhibitOS Manager.app')))!==JSON.stringify(await resourceInventory(destination)))fail('OFFICIAL_MOUNTED_APP_CHANGED');await run('/usr/bin/codesign',['--verify','--deep','--strict',join(mount,'ExhibitOS Manager.app')]);}
  finally{if(attached){await run('/usr/bin/hdiutil',['detach',mount]);attached=false;}}
  s.state='signed';s.dmg=dmg;s.dmgSha256=sha((await regular(dmg)).bytes);s.app=destination;s.sourceAppInventorySha256=sha(Buffer.from(JSON.stringify(before)));s.signedAppInventorySha256=sha(Buffer.from(JSON.stringify(await resourceInventory(destination))));await save(join(p.workDir,'operation.json'),s);return {state:s.state,dmg,sha256:s.dmgSha256};
 }catch(error){s.state='finishing-failed';s.code=/^OFFICIAL_/.test(error.message)?error.message:'OFFICIAL_FINISH_FAILED';await save(join(p.workDir,'operation.json'),s);throw Error(s.code);}
}
export async function notarizeOfficial(p,options={}){
 validateOfficialPlan(p,options.repo??repoRoot);if(!options.execute)fail('OFFICIAL_EXPLICIT_EXECUTION_REQUIRED');await directory(p.workDir);
 const path=join(p.workDir,'notary-operation.lock');let lock;
 try{lock=await open(path,'wx',0o600);}catch(error){if(error.code==='EEXIST')fail('OFFICIAL_OPERATION_BUSY');throw error;}
 const identity=await lock.stat();
 try{await lock.writeFile(JSON.stringify({pid:process.pid}));return await notarizeUnlocked(p,options);}
 finally{await lock.close();const named=await lstat(path);if(named.dev===identity.dev&&named.ino===identity.ino)await unlink(path);}
}
async function notarizeUnlocked(p,{repo=repoRoot,run=command,execute=false}={}){
 validateOfficialPlan(p,repo);if(!execute)fail('OFFICIAL_EXPLICIT_EXECUTION_REQUIRED');let s=await stateFor(p);const file=join(p.workDir,'operation.json');
 if(s.dmg!==join(p.workDir,'ExhibitOS-Manager.dmg')||s.app!==join(p.workDir,'signed-app/ExhibitOS Manager.app'))fail('OFFICIAL_OPERATION_CHANGED');
 if(!['signed','submitted','ticket-accepted','stapled','accepted'].includes(s.state))fail('OFFICIAL_STATE_REFUSED');
 if(sha((await regular(s.dmg)).bytes)!==(['stapled','accepted'].includes(s.state)?s.finalSha256:s.dmgSha256))fail('OFFICIAL_ARTIFACT_CHANGED');
 if(s.state==='accepted')return {state:s.state,sha256:s.finalSha256};
 if(s.state==='signed'){
  s.state='submission-uncertain';await save(file,s);
  const reply=JSON.parse(await run('/usr/bin/xcrun',['notarytool','submit',s.dmg,'--keychain-profile',p.notaryProfile,'--output-format','json']));if(!uuid(reply.id))fail('OFFICIAL_SUBMISSION_UNCERTAIN');s.submissionId=reply.id;s.state='submitted';await save(file,s);if(sha((await regular(s.dmg)).bytes)!==s.dmgSha256)fail('OFFICIAL_ARTIFACT_CHANGED');return {state:s.state,submissionId:s.submissionId};
 }
 if(!uuid(s.submissionId))fail('OFFICIAL_SUBMISSION_UNCERTAIN');
 if(s.state==='submitted'){
  const info=JSON.parse(await run('/usr/bin/xcrun',['notarytool','info',s.submissionId,'--keychain-profile',p.notaryProfile,'--output-format','json']));if(info.id!==s.submissionId)fail('OFFICIAL_SUBMISSION_CHANGED');
  if(!['Accepted','Invalid','Rejected'].includes(info.status))return {state:'submitted',submissionId:s.submissionId};
  s.logAttempts=(s.logAttempts??0)+1;const log=join(p.workDir,'notary-log-'+s.submissionId+'-'+s.logAttempts+'.json');s.log=log;await save(file,s);await run('/usr/bin/xcrun',['notarytool','log',s.submissionId,'--keychain-profile',p.notaryProfile,log]);s.notaryStatus=info.status;s.state=info.status==='Accepted'?'ticket-accepted':'rejected';await save(file,s);
  if(s.state==='rejected')return {state:s.state,submissionId:s.submissionId};
 }
 if(s.state==='ticket-accepted'){
  s.state='staple-uncertain';await save(file,s);await run('/usr/bin/xcrun',['stapler','staple',s.dmg]);s.finalSha256=sha((await regular(s.dmg)).bytes);s.state='stapled';await save(file,s);
 }
 await run('/usr/bin/xcrun',['stapler','validate',s.dmg]);await run('/usr/sbin/spctl',['--assess','--type','open','--context','context:primary-signature',s.dmg]);await run('/usr/sbin/spctl',['--assess','--type','execute',s.app]);s.state='accepted';await save(file,s);return {state:s.state,submissionId:s.submissionId,sha256:s.finalSha256};
}

if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{const [action,planFile,flag,...extra]=process.argv.slice(2);if(extra.length||!['plan','finish','notarize'].includes(action)||!absolute(planFile))fail('OFFICIAL_ARGUMENTS_INVALID');const p=validateOfficialPlan(await json(planFile));if(action==='plan'){if(flag)fail('OFFICIAL_ARGUMENTS_INVALID');console.log(JSON.stringify({state:'PLAN_ONLY_NO_CREDENTIAL_ACCESS',source:p.sourceCommit,workDir:p.workDir,appPath:p.appPath,actions:['nested-runtime-before-compile','finish-copy-and-DMG','notary-submit-and-resume'],officialExecutionApproved:false}));}else{if(flag!=='--execute-official-distribution')fail('OFFICIAL_EXPLICIT_EXECUTION_REQUIRED');console.log(JSON.stringify(await (action==='finish'?finishOfficialApp:notarizeOfficial)(p,{execute:true})));}}catch(error){console.error(JSON.stringify({code:/^OFFICIAL_/.test(error.message)?error.message:'OFFICIAL_FAILED',productionSigned:false,notarized:false}));process.exitCode=1;}
}
