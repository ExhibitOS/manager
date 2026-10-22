// SPDX-License-Identifier: Apache-2.0
// Shared runtime admission has no CLI entry point.
import {constants} from 'node:fs';
import {lstat,open,realpath} from 'node:fs/promises';
import {dirname,join} from 'node:path';
import {QUALIFIED_REDISTRIBUTION,matchesDistributionFiles} from './update-runtime-distribution.mjs';
import {SOURCE} from './prepare-update-runtime.mjs';
const fail=code=>{throw Error(code);};
const same=(a,b)=>['dev','ino','size','mtimeNs','ctimeNs','mode','nlink'].every(k=>a[k]===b[k]);
const manifestLimit=2*1024**2;
export async function assertOfficialRuntime(repo){
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
  if(manifest?.format!=='exhibitos-manager-update-runtime-v1'||manifest.version!==1)fail('DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED');
  if(manifest.redistribution==='private-validation-only')fail('DESKTOP_PRIVATE_RUNTIME_OFFICIAL_RELEASE_REFUSED');
  if(Object.keys(manifest).sort().join(',')!=='executable,files,format,platform,pythonVersion,redistribution,selection,source,version'||manifest.redistribution!==QUALIFIED_REDISTRIBUTION||manifest.platform!=='darwin-arm64'||manifest.pythonVersion!=='3.11.17'||manifest.executable!=='python/bin/python3.11'||manifest.selection!=='full-stdlib-no-static-embedding-v1'||!manifest.source||Object.keys(manifest.source).sort().join(',')!==Object.keys(SOURCE).sort().join(',')||Object.entries(SOURCE).some(([k,v])=>manifest.source[k]!==v)||!matchesDistributionFiles(manifest.files))fail('DESKTOP_RELEASE_RUNTIME_UNRECOGNIZED');
  // This prebuild check admits only the fixed producer contract. The native
  // consumer still binds its compiled manifest and verifies every physical byte,
  // path, mode and identity before using Python; this is not signature evidence.
  return;
 }catch(error){if(error.code==='ENOENT')fail('DESKTOP_RELEASE_RUNTIME_MISSING');if(/^DESKTOP_[A-Z_]+$/.test(error.message))throw error;fail('DESKTOP_RELEASE_RUNTIME_UNSAFE');}
}
