// SPDX-License-Identifier: Apache-2.0
// Preserve the public locked production tree: spec validators load files at runtime.
import {readFile,writeFile,mkdir,lstat,readdir,rename,rm} from 'node:fs/promises';
import {createHash,randomUUID} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {dirname,join,resolve,posix} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const sha=b=>createHash('sha256').update(b).digest('hex');
const json=async p=>JSON.parse(await readFile(p,'utf8'));
const safe=p=>typeof p==='string'&&!p.startsWith('/')&&!p.split('/').some(s=>!s||s==='..'||s==='.'||s.startsWith('.'));
export const allowedViewerPath=p=>['static.html','THIRD_PARTY_NOTICES.txt','voice-pcm-worklet.js'].includes(p)||/^(assets|basis)\/[A-Za-z0-9_.-]+\.(js|css|wasm)$/.test(p);
export async function preparePublicationRuntime({platformRoot,output,viewerRoot}={}){
 if(process.version!=='v24.21.0')throw Error('PUBLICATION_NODE_VERSION');
 if(!platformRoot||!output)throw Error('PUBLICATION_PREPARE_PATHS_REQUIRED');
 const root=resolve(platformRoot),target=resolve(output),viewer=resolve(viewerRoot??join(root,'apps/web/dist-static'));
 if(target===root||['apps','packages','node_modules','scripts','vendor'].some(p=>target===join(root,p)||target.startsWith(join(root,p)+'/'))||target===viewer||target.startsWith(viewer+'/'))throw Error('PUBLICATION_OUTPUT_UNSAFE');
 try{await lstat(target);throw Error('PUBLICATION_OUTPUT_EXISTS');}catch(e){if(e.code!=='ENOENT')throw e;}
 const viewerStat=await lstat(viewer);if(!viewerStat.isDirectory()||viewerStat.isSymbolicLink())throw Error('PUBLICATION_VIEWER_DIRECTORY');
 const lock=await json(join(root,'package-lock.json'));
 if(lock.lockfileVersion!==3)throw Error('PUBLICATION_LOCK_VERSION');
 const commit=execFileSync('git',['-C',root,'rev-parse','HEAD'],{encoding:'utf8'}).trim();
 // Only tracked public files enter the source archive; no working directory, keys or private trees.
 const tracked=execFileSync('git',['-C',root,'ls-files','-z'],{encoding:'utf8',maxBuffer:16*1024**2}).split('\0').filter(Boolean);
 if(tracked.some(p=>/^(operations|\.local|Capture)(\/|$)/i.test(p)))throw Error('PUBLICATION_PRIVATE_SOURCE_TREE');
 if(execFileSync('git',['-C',root,'status','--porcelain','--untracked-files=no'],{encoding:'utf8'}).trim())throw Error('PUBLICATION_SOURCE_DIRTY');
 const artifact=await json(join(root,'contracts/artifact.json'));
 if(!safe(artifact.file)||sha(await readFile(join(root,artifact.file)))!==artifact.sha256)throw Error('PUBLICATION_SPEC_PIN');
 const staging=target+'.preparing-'+randomUUID(),inventory={},links={},packages=new Map();
 await mkdir(dirname(target),{recursive:true});await mkdir(staging,{mode:0o700});
 async function put(name,bytes){if(!safe(name))throw Error('PUBLICATION_PATH');await mkdir(dirname(join(staging,name)),{recursive:true});await writeFile(join(staging,name),bytes,{flag:'wx',mode:0o600});inventory[name]={bytes:bytes.length,sha256:sha(bytes)};}
 async function copy(source,destination,{filter=()=>true,omitModules=false}={}){
  const stat=await lstat(source);if(stat.isSymbolicLink())throw Error('PUBLICATION_SYMLINK: '+destination);
  if(stat.isDirectory()){for(const name of (await readdir(source)).sort()){if(omitModules&&name==='node_modules')continue;if(name.startsWith('.'))continue;await copy(join(source,name),destination+'/'+name,{filter,omitModules});}}
  else if(stat.isFile()){if(!filter(destination))throw Error('PUBLICATION_VIEWER_UNLISTED: '+destination);if(stat.size>100*1024**2)throw Error('PUBLICATION_FILE_BUDGET');await put(destination,await readFile(source));}
  else throw Error('PUBLICATION_FILE_TYPE');
 }
 function locate(name,from){let base=from;for(;;){const key=posix.join(base,'node_modules',name);if(lock.packages[key])return key;if(!base)break;base=posix.dirname(base);if(base==='.')base='';}throw Error('PUBLICATION_DEPENDENCY_MISSING: '+name);}
 async function dependency(key){
  if(packages.has(key))return;const entry=lock.packages[key];if(!entry)throw Error('PUBLICATION_LOCK_ENTRY');
  packages.set(key,entry);
  if(entry.link){if(!safe(entry.resolved)||!['packages/storage','packages/studio-contract'].includes(entry.resolved))throw Error('PUBLICATION_WORKSPACE');await workspace(entry.resolved);await copy(join(staging,'platform',entry.resolved),'platform/'+key);return;}
  const actual=await json(join(root,key,'package.json'));if(actual.version!==entry.version)throw Error('PUBLICATION_INSTALLED_VERSION: '+key);
  await copy(join(root,key),'platform/'+key,{omitModules:true});
  for(const name of Object.keys({...entry.dependencies,...entry.optionalDependencies}).sort()){
   let child;try{child=locate(name,key);}catch(e){if(entry.optionalDependencies?.[name])continue;throw e;}
   try{await lstat(join(root,child));}catch(e){if(e.code==='ENOENT'&&entry.optionalDependencies?.[name])continue;throw e;}
   await dependency(child);
  }
 }
 async function workspace(key){
  if(packages.has('workspace:'+key))return;packages.set('workspace:'+key,lock.packages[key]);
  await copy(join(root,key,'package.json'),'platform/'+key+'/package.json');await copy(join(root,key,'dist'),'platform/'+key+'/dist');
  for(const name of Object.keys(lock.packages[key].dependencies??{}).sort())await dependency(locate(name,key));
 }
 try{
  await workspace('apps/api');
  await copy(join(root,'scripts/export-static-publication.mjs'),'platform/scripts/export-static-publication.mjs');
  await copy(join(root,'package.json'),'platform/package.json');await copy(join(root,'package-lock.json'),'platform/package-lock.json');
  await copy(join(root,'contracts/artifact.json'),'platform/contracts/artifact.json');
  await copy(join(root,'LICENSE'),'LICENSE-PLATFORM');await copy(join(root,artifact.file),'source/'+posix.basename(artifact.file));
  // Reject unfiltered web output instead of silently dropping unknown entries.
  await copy(viewer,'viewer',{filter:p=>allowedViewerPath(p.slice('viewer/'.length))});
  for(const required of ['static.html','THIRD_PARTY_NOTICES.txt','voice-pcm-worklet.js'])if(!inventory['viewer/'+required])throw Error('PUBLICATION_VIEWER_REQUIRED: '+required);
  if((await readFile(join(staging,'viewer/static.html'),'utf8')).split('STATIC_INDEX_SHA256').length!==2)throw Error('PUBLICATION_VIEWER_INDEX');
  for(const required of ['validators/artwork.mjs','validators/exhibition.mjs','validators/package.mjs','oes/v1/artwork.schema.json','oes/v1/exhibition.schema.json','oes/v1/lifecycle.schema.json','oes/v1/spatial-scripting.schema.json','oex/v1/manifest.schema.json','oex/v1/media-manifest.schema.json','oed/v1/deployment.schema.json','schemas/artwork.json','LICENSE'])if(!inventory['platform/node_modules/@exhibitos/spec/'+required])throw Error('PUBLICATION_SPEC_ASSET: '+required);
  if(![...Object.keys(inventory)].some(p=>p.endsWith('/gltf-validator/gltf_validator.dart.js')))throw Error('PUBLICATION_GLTF_VALIDATOR_ASSET');
  await put('exporter.mjs',Buffer.from("// SPDX-License-Identifier: AGPL-3.0-or-later\nexport {exportStaticPublication} from './platform/scripts/export-static-publication.mjs';\n"));
  const archive=execFileSync('git',['-C',root,'archive','--format=tar',commit],{maxBuffer:100*1024**2});await put('source/platform-'+commit+'.tar',archive);
  const manifest={version:1,format:'exhibitos-manager-publication-runtime-v1',platformCommit:commit,lockSha256:sha(await readFile(join(root,'package-lock.json'))),spec:artifact,nodeVersion:'24.21.0',entry:'exporter.mjs',viewer:'viewer',networkAcquisition:false,packages:[...packages].map(([path,p])=>({path,version:p.version??null,integrity:p.integrity??null,resolved:p.resolved??null})),links,files:inventory};
  await put('manifest.json',Buffer.from(JSON.stringify(manifest)+'\n'));
  // Run only an import check in a fresh Node process: this cannot reuse the source tree's module cache.
  execFileSync(process.execPath,['--input-type=module','-e','const m=await import(process.argv[1]);if(typeof m.exportStaticPublication!=="function")throw Error("PUBLICATION_EXPORT_INTERFACE")',pathToFileURL(join(staging,'exporter.mjs')).href],{cwd:staging,timeout:30000,stdio:'pipe',env:{PATH:dirname(process.execPath)}});
  await rename(staging,target);return {prepared:true,platformCommit:commit,files:Object.keys(inventory).length,bytes:Object.values(inventory).reduce((n,f)=>n+f.bytes,0),packages:packages.size,workspaceLinks:Object.keys(links).length,output:target,networkAcquisition:false};
 }catch(e){await rm(staging,{recursive:true,force:true});throw e;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 const [platformRoot,output,viewerRoot]=process.argv.slice(2);
 if(!platformRoot)throw Error('Usage: prepare-publication-runtime.mjs PUBLIC_PLATFORM_CHECKOUT [NEW_OUTPUT] [DIST_STATIC]');
 console.log(JSON.stringify(await preparePublicationRuntime({platformRoot,output:output??fileURLToPath(new URL('../src-tauri/publication-runtime/generated',import.meta.url)),viewerRoot})));
}
