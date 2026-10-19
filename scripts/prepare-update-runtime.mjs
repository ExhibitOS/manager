// SPDX-License-Identifier: Apache-2.0
// Build-time only. No network, PATH Python lookup, pip, user profile or source tree input.
import {open,lstat,realpath,statfs,mkdir,writeFile,chmod} from 'node:fs/promises';
import {constants} from 'node:fs';
import {createHash} from 'node:crypto';
import {zstdDecompressSync} from 'node:zlib';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {inspectTar,selectRuntime} from './update-runtime-archive.mjs';
export const SOURCE=Object.freeze({release:'20261013',archiveName:'cpython-3.11.17+20261013-aarch64-apple-darwin-pgo+lto-full.tar.zst',url:'https://github.com/astral-sh/python-build-standalone/releases/download/20261013/cpython-3.11.17%2B20261013-aarch64-apple-darwin-pgo%2Blto-full.tar.zst',sha256:'4e2b20716a080e9141ea80029dabbdc9457e20760f3fd3b16a17abd48ea6bf4a',bytes:42796019});
const hash=b=>createHash('sha256').update(b).digest('hex');
const refuse=code=>{throw Error('UPDATE_RUNTIME_'+code);};
function identity(s){return [s.dev,s.ino,s.mode,s.nlink,s.uid,s.gid,s.size,s.mtimeNs,s.ctimeNs].map(String).join(':');}
async function directory(path){if(await realpath(path)!==path||!(await lstat(path)).isDirectory())refuse('DIRECTORY_UNSAFE');}
export async function prepareUpdateRuntime({archive,output}){
 if(process.platform!=='darwin'||process.arch!=='arm64')refuse('HOST_UNSUPPORTED');
 if(!archive||!output||resolve(archive)!==archive||resolve(output)!==output)refuse('ABSOLUTE_INPUT_REQUIRED');await directory(dirname(output));
 try{await lstat(output);refuse('OUTPUT_EXISTS');}catch(e){if(e.code!=='ENOENT')throw e;}
 const space=await statfs(dirname(output),{bigint:true});if(space.bavail*space.bsize<1073741824n+19283124845n+6442450944n)refuse('SPACE_ADMISSION_FAILED');
 if(await realpath(archive)!==archive)refuse('ARCHIVE_UNSAFE');const before=await lstat(archive,{bigint:true});if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1n||before.size!==BigInt(SOURCE.bytes))refuse('ARCHIVE_UNSAFE');
 const fd=await open(archive,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);let bytes;
 try{const opened=await fd.stat({bigint:true});if(identity(before)!==identity(opened))refuse('ARCHIVE_CHANGED');bytes=Buffer.alloc(SOURCE.bytes+1);let count=0;while(count<bytes.length){const {bytesRead}=await fd.read(bytes,count,bytes.length-count,null);if(!bytesRead)break;count+=bytesRead;}if(count!==SOURCE.bytes)refuse('ARCHIVE_CHANGED');bytes=bytes.subarray(0,count);if(bytes.length!==SOURCE.bytes||hash(bytes)!==SOURCE.sha256)refuse('ARCHIVE_CHECKSUM');if(identity(before)!==identity(await fd.stat({bigint:true}))||identity(before)!==identity(await lstat(archive,{bigint:true})))refuse('ARCHIVE_CHANGED');}finally{await fd.close();}
 const tar=zstdDecompressSync(bytes,{maxOutputLength:256*1024**2});const tree=inspectTar(tar);const selected=selectRuntime(tree);const metadata=JSON.parse(selected.get('metadata/PYTHON.json').data.toString('utf8'));
 if(metadata.target_triple!=='aarch64-apple-darwin'||metadata.python_version!=='3.11.17'||metadata.apple_sdk_deployment_target!=='11.0')refuse('METADATA_MISMATCH');
 const declared=new Set();function licenses(v){if(v&&typeof v==='object'){for(const [k,x]of Object.entries(v)){if(k==='license_path'||k==='license_paths')for(const p of typeof x==='string'?[x]:x){if(typeof p!=='string'||!p.startsWith('licenses/')||p.split('/').some(s=>!s||s==='..'||s==='.'))refuse('LICENSE_METADATA_UNSAFE');declared.add(p);}licenses(x);}}}licenses(metadata);
 const missing=[...declared].filter(n=>!tree.entries.has('python/'+n)).sort();if(JSON.stringify(missing)!=='["licenses/LICENSE.zlib-ng.txt"]')refuse('LICENSE_METADATA_CHANGED');
 const discrepancy={status:'private-validation-only-not-public-redistribution-cleared',upstreamRelease:SOURCE.release,declaredMissing:missing,observedDependency:'Darwin zlib metadata uses system z; inspected fixed artifact main Mach-O loads /usr/lib/libz.1.dylib; LICENSE.zlib.txt is included',upstreamSource:'https://github.com/astral-sh/python-build-standalone/blob/20261013/pythonbuild/downloads.json',metadataCause:'Pinned pythonbuild/utils.py add_licenses_to_extension_entry matches every DOWNLOADS entry by library_names: both zlib and zlib-ng declare z; the Darwin archive links system z and has no bundled zlib-ng library',metadataGenerator:'https://github.com/astral-sh/python-build-standalone/blob/20261013/pythonbuild/utils.py#L556-L595',limitation:'The tag declares zlib-ng as an alternate dependency but has no LICENSE.zlib-ng.txt. This preserves the discrepancy and does not assert redistribution clearance.'};
 selected.set('metadata/license-discrepancy.json',{data:Buffer.from(JSON.stringify(discrepancy,null,2)+'\n'),mode:0o644});
 let sum=0;for(const v of selected.values())sum+=v.data.length;if(sum>128*1024**2)refuse('OUTPUT_LIMIT');await mkdir(output,{mode:0o700});const files={};
 try{for(const [name,v]of [...selected].sort(([a],[b])=>a<b?-1:a>b?1:0)){const p=join(output,name);await mkdir(dirname(p),{recursive:true,mode:0o755});await writeFile(p,v.data,{flag:'wx',mode:v.mode});await chmod(p,v.mode);files[name]={sha256:hash(v.data),bytes:v.data.length,mode:v.mode};}
 const manifest={format:'exhibitos-manager-update-runtime-v1',version:1,platform:'darwin-arm64',pythonVersion:'3.11.17',executable:'python/bin/python3.11',selection:'full-stdlib-no-static-embedding-v1',source:SOURCE,redistribution:'private-validation-only',files};await writeFile(join(output,'manifest.json'),JSON.stringify(manifest)+'\n',{flag:'wx',mode:0o644});await chmod(join(output,'manifest.json'),0o644);
 return{prepared:true,output,networkAcquisition:false,pythonExecuted:false,signatureChanged:false,bytes:sum,files:Object.keys(files).length,manifestSha256:hash(Buffer.from(JSON.stringify(manifest)+'\n')),redistribution:manifest.redistribution};
 }catch(e){throw Error('UPDATE_RUNTIME_PARTIAL_OUTPUT_PRESERVED',{cause:e});}
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const a=process.argv.slice(2);if(a.length!==4||a[0]!=='--archive'||a[2]!=='--output')refuse('USAGE');console.log(JSON.stringify(await prepareUpdateRuntime({archive:a[1],output:a[3]})));
}
