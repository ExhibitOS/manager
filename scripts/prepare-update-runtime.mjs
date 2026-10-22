// SPDX-License-Identifier: Apache-2.0
// Build-time only. No network, PATH Python lookup, pip, user profile or source tree input.
import {open,lstat,realpath,statfs,mkdir,writeFile,chmod} from 'node:fs/promises';
import {constants,readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {zstdDecompressSync} from 'node:zlib';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {inspectTar,selectRuntime} from './update-runtime-archive.mjs';
import {qualifyRuntimeSelection,QUALIFIED_REDISTRIBUTION} from './update-runtime-distribution.mjs';
export const SOURCE=Object.freeze({release:'20261013',archiveName:'cpython-3.11.17+20261013-aarch64-apple-darwin-pgo+lto-full.tar.zst',url:'https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.11.17%2B20261013-aarch64-apple-darwin-pgo%2Blto-full.tar.zst',sha256:'4e2b20716a080e9141ea80029dabbdc9457e20760f3fd3b16a17abd48ea6bf4a',bytes:42796019});
// Independent notice supplement for this exact upstream declaration, not an assertion
// that zlib-ng is linked or that the runtime is publicly qualified.
export const NOTICE_SOURCE=Object.freeze({repository:'https://github.com/python/cpython-source-deps',tag:'zlib-ng-2.2.4',commit:'a502e047655c31db29601def76ad6539acacf43d',path:'LICENSE.md',url:'https://raw.githubusercontent.com/python/cpython-source-deps/a502e047655c31db29601def76ad6539acacf43d/LICENSE.md',bytes:867,sha256:'6c9f0d975b41afaa34d22f55bb8986ce69e5cb7ad327cb2b28820cd425edf5ee'});
const hash=b=>createHash('sha256').update(b).digest('hex');
const refuse=code=>{throw Error('UPDATE_RUNTIME_'+code);};
function identity(s){return [s.dev,s.ino,s.mode,s.nlink,s.uid,s.gid,s.size,s.mtimeNs,s.ctimeNs].map(String).join(':');}
async function directory(path){if(await realpath(path)!==path||!(await lstat(path)).isDirectory())refuse('DIRECTORY_UNSAFE');}
/** Pure, bounded selection check; never modifies archive metadata or existing notices. */
export function completeRuntimeNotices(tree,selected,notice,descriptor){
 if(!descriptor||Object.keys(descriptor).sort().join(',')!==Object.keys(NOTICE_SOURCE).sort().join(',')||Object.entries(NOTICE_SOURCE).some(([key,value])=>descriptor[key]!==value))refuse('NOTICE_SOURCE_CHANGED');
 if(!Buffer.isBuffer(notice)||notice.length!==NOTICE_SOURCE.bytes||hash(notice)!==NOTICE_SOURCE.sha256)refuse('NOTICE_BYTES_CHANGED');
 const original=tree.regular('python/PYTHON.json').data,metadataBytes=selected.get('metadata/PYTHON.json')?.data;
 if(!Buffer.isBuffer(metadataBytes)||!metadataBytes.equals(original))refuse('LICENSE_METADATA_CHANGED');
 const metadata=JSON.parse(metadataBytes.toString('utf8')),declared=new Set();
 function licenses(v){if(v&&typeof v==='object'){for(const [key,value]of Object.entries(v)){
  if(key==='license_path'||key==='license_paths'){
   const paths=typeof value==='string'?[value]:value;
   if(!Array.isArray(paths)||!paths.length)refuse('LICENSE_METADATA_UNSAFE');
   for(const path of paths){if(typeof path!=='string'||!path.startsWith('licenses/')||path.includes('\\')||path.includes('\0')||path.split('/').some(part=>!part||part==='..'||part==='.'))refuse('LICENSE_METADATA_UNSAFE');declared.add(path);}
  }
  licenses(value);
 }}}
 licenses(metadata);
 const missing=[...declared].filter(name=>!tree.entries.has('python/'+name)).sort();
 if(JSON.stringify(missing)!=='["licenses/LICENSE.zlib-ng.txt"]')refuse('LICENSE_METADATA_CHANGED');
 const resolved=new Map(selected),supplementPath='notices/LICENSE.zlib-ng.txt';
 if(resolved.has(supplementPath))refuse('NOTICE_SOURCE_CHANGED');
 resolved.set(supplementPath,{data:Buffer.from(notice),mode:0o644});
 const correspondence=[];
 for(const licensePath of [...declared].sort()){
  const selectedPath='notices/'+licensePath.slice('licenses/'.length),actual=resolved.get(selectedPath)?.data;
  const supplemented=licensePath===missing[0],expected=supplemented?notice:tree.regular('python/'+licensePath).data;
  if(!Buffer.isBuffer(actual)||!actual.length||!actual.toString('utf8').trim()||!actual.equals(expected))refuse('NOTICE_CORRESPONDENCE_CHANGED');
  correspondence.push({licensePath,selectedPath,bytes:actual.length,sha256:hash(actual),origin:supplemented?'explicit-upstream-supplement':'unchanged-archive'});
 }
 return{selected:resolved,discrepancy:{status:'upstream-missing-notice-explicitly-supplemented',upstreamRelease:SOURCE.release,declaredMissing:missing,declaredMissingInOriginalArchive:missing,supplement:{selectedPath:supplementPath,source:NOTICE_SOURCE},declaredNoticeFiles:correspondence,metadataPreserved:true,limitation:'The original archive omits a declared notice. The exact upstream notice is supplied separately without claiming which library is linked, legal clearance or public runtime qualification.'}};
}
export async function prepareUpdateRuntime({archive,output}){
 if(process.platform!=='darwin'||process.arch!=='arm64')refuse('HOST_UNSUPPORTED');
 if(!archive||!output||resolve(archive)!==archive||resolve(output)!==output)refuse('ABSOLUTE_INPUT_REQUIRED');await directory(dirname(output));
 try{await lstat(output);refuse('OUTPUT_EXISTS');}catch(e){if(e.code!=='ENOENT')throw e;}
 const space=await statfs(dirname(output),{bigint:true});if(space.bavail*space.bsize<1073741824n+19283124845n+6442450944n)refuse('SPACE_ADMISSION_FAILED');
 if(await realpath(archive)!==archive)refuse('ARCHIVE_UNSAFE');const before=await lstat(archive,{bigint:true});if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1n||before.size!==BigInt(SOURCE.bytes))refuse('ARCHIVE_UNSAFE');
 const fd=await open(archive,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);let bytes;
 try{const opened=await fd.stat({bigint:true});if(identity(before)!==identity(opened))refuse('ARCHIVE_CHANGED');bytes=Buffer.alloc(SOURCE.bytes+1);let count=0;while(count<bytes.length){const {bytesRead}=await fd.read(bytes,count,bytes.length-count,null);if(!bytesRead)break;count+=bytesRead;}if(count!==SOURCE.bytes)refuse('ARCHIVE_CHANGED');bytes=bytes.subarray(0,count);if(bytes.length!==SOURCE.bytes||hash(bytes)!==SOURCE.sha256)refuse('ARCHIVE_CHECKSUM');if(identity(before)!==identity(await fd.stat({bigint:true}))||identity(before)!==identity(await lstat(archive,{bigint:true})))refuse('ARCHIVE_CHANGED');}finally{await fd.close();}
 const tar=zstdDecompressSync(bytes,{maxOutputLength:256*1024**2});const tree=inspectTar(tar),originalSelection=selectRuntime(tree);const metadata=JSON.parse(originalSelection.get('metadata/PYTHON.json').data.toString('utf8'));
 if(metadata.target_triple!=='aarch64-apple-darwin'||metadata.python_version!=='3.11.17'||metadata.apple_sdk_deployment_target!=='11.0')refuse('METADATA_MISMATCH');
 const notice=readFileSync(new URL('./update-runtime-notices/LICENSE.zlib-ng.txt',import.meta.url));
 const descriptor=JSON.parse(readFileSync(new URL('./update-runtime-notices/source.json',import.meta.url),'utf8'));
 const completed=completeRuntimeNotices(tree,originalSelection,notice,descriptor);
 const selected=qualifyRuntimeSelection(completed.selected),discrepancy=completed.discrepancy;
 selected.set('metadata/license-discrepancy.json',{data:Buffer.from(JSON.stringify(discrepancy,null,2)+'\n'),mode:0o644});
 let sum=0;for(const v of selected.values())sum+=v.data.length;if(sum>128*1024**2)refuse('OUTPUT_LIMIT');await mkdir(output,{mode:0o700});const files={};
 try{for(const [name,v]of [...selected].sort(([a],[b])=>a<b?-1:a>b?1:0)){const p=join(output,name);await mkdir(dirname(p),{recursive:true,mode:0o755});await writeFile(p,v.data,{flag:'wx',mode:v.mode});await chmod(p,v.mode);files[name]={sha256:hash(v.data),bytes:v.data.length,mode:v.mode};}
 const manifest={format:'exhibitos-manager-update-runtime-v1',version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:QUALIFIED_REDISTRIBUTION,files};await writeFile(join(output,'manifest.json'),JSON.stringify(manifest)+'\n',{flag:'wx',mode:0o644});await chmod(join(output,'manifest.json'),0o644);
 return{prepared:true,output,networkAcquisition:false,pythonExecuted:false,signatureChanged:false,bytes:sum,files:Object.keys(files).length,manifestSha256:hash(Buffer.from(JSON.stringify(manifest)+'\n')),redistribution:manifest.redistribution};
 }catch(e){throw Error('UPDATE_RUNTIME_PARTIAL_OUTPUT_PRESERVED',{cause:e});}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const a=process.argv.slice(2);if(a.length!==4||a[0]!=='--archive'||a[2]!=='--output')refuse('USAGE');console.log(JSON.stringify(await prepareUpdateRuntime({archive:a[1],output:a[3]})));
}
