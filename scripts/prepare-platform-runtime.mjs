// SPDX-License-Identifier: Apache-2.0
// Operator packaging only. Never builds images or obtains credentials.
import {createReadStream} from 'node:fs';
import {mkdir,lstat,readFile,copyFile,chmod,rename,readdir} from 'node:fs/promises';
import {createHash,randomUUID} from 'node:crypto';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const fail=code=>{throw Error(code);};
const digest=async p=>{const h=createHash('sha256');for await(const b of createReadStream(p))h.update(b);return h.digest('hex');};
async function regular(p){const s=await lstat(p);if(!s.isFile()||s.isSymbolicLink()||s.nlink!==1)fail('PLATFORM_RUNTIME_FILE_UNSAFE');return s;}
export async function preparePlatformRuntime({bundle,output,architecture,platformCommit}={}){
 if(!bundle||!output||!['arm64','x86_64'].includes(architecture)||!(/^[a-f0-9]{40}$/.test(platformCommit)))fail('PLATFORM_RUNTIME_INPUT_REQUIRED');
 const source=resolve(bundle),target=resolve(output);if(source===target||target.startsWith(source+'/'))fail('PLATFORM_RUNTIME_OUTPUT_UNSAFE');
 const ds=await lstat(source);if(!ds.isDirectory()||ds.isSymbolicLink())fail('PLATFORM_RUNTIME_DIRECTORY_UNSAFE');
 try{await lstat(target);fail('PLATFORM_RUNTIME_OUTPUT_EXISTS');}catch(e){if(e.code!=='ENOENT')throw e;}
 await regular(join(source,'manifest.json'));const manifest=JSON.parse(await readFile(join(source,'manifest.json'),'utf8'));
 if(manifest.schemaVersion!=='1.0.0-draft.1'||manifest.protocolVersion!=='1'||!Array.isArray(manifest.images)||!manifest.images.length)fail('PLATFORM_RUNTIME_MANIFEST_INVALID');
 const names=['manifest.json','compose.yaml',...manifest.images.flatMap(i=>i.archive?[i.archive.path]:[])];
 if(new Set(names).size!==names.length||names.some(n=>!(/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(n))))fail('PLATFORM_RUNTIME_FILE_UNSAFE');
 const actual=await readdir(source);if(actual.length!==names.length||actual.some(n=>!names.includes(n)))fail('PLATFORM_RUNTIME_UNEXPECTED_FILE');
 const compose=JSON.parse(await readFile(join(source,'compose.yaml'),'utf8'));if(!compose||compose.name!==manifest.projectName)fail('PLATFORM_RUNTIME_COMPOSE_INVALID');
 const files={};let total=0;
 for(const name of names){const p=join(source,name),s=await regular(p);if(s.size<=0||s.size>8*1024**3||(name==='manifest.json'&&s.size>65536)||(name==='compose.yaml'&&s.size>256*1024))fail('PLATFORM_RUNTIME_FILE_SIZE');files[name]={bytes:s.size,sha256:await digest(p)};total+=s.size;}
 if(files['compose.yaml'].sha256!==manifest.composeSha256||manifest.images.some(i=>i.archive&&(files[i.archive.path].bytes!==i.archive.bytes||files[i.archive.path].sha256!==i.archive.sha256)))fail('PLATFORM_RUNTIME_HASH_MISMATCH');
 const receipt={format:'exhibitos-manager-platform-runtime-v1',version:manifest.version,architecture,platformCommit,files};
 await mkdir(dirname(target),{recursive:true});const stage=target+'.preparing-'+randomUUID();await mkdir(stage,{mode:0o700});
 // Partial output is preserved on failure; no existing output or source is replaced.
 for(const name of names){const p=join(source,name);await copyFile(p,join(stage,name),1);await chmod(join(stage,name),0o600);if(await digest(join(stage,name))!==files[name].sha256||await digest(p)!==files[name].sha256)fail('PLATFORM_RUNTIME_SOURCE_CHANGED');}
 const {writeFile}=await import('node:fs/promises');await writeFile(join(stage,'prepared.json'),JSON.stringify(receipt)+'\n',{flag:'wx',mode:0o600});await rename(stage,target);
 return {prepared:true,architecture,platformCommit,files:names.length,bytes:total,networkAcquisition:false,distribution:'operator-prepared-resource; publisher authenticity requires trusted application distribution'};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){const [bundle,architecture,platformCommit,output]=process.argv.slice(2);console.log(JSON.stringify(await preparePlatformRuntime({bundle,architecture,platformCommit,output:output??fileURLToPath(new URL('../src-tauri/platform-runtime/generated',import.meta.url))})));}
