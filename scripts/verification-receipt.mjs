// SPDX-License-Identifier: Apache-2.0
import {createHash} from 'node:crypto';
import {lstat,open} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';

const same=(a,b)=>['dev','ino','size','mtimeNs','ctimeNs'].every(key=>a[key]===b[key]);
function sourceReceipt(source) {
  const sourceCommit=execFileSync('git',['rev-parse','HEAD'],{cwd:source,encoding:'utf8',windowsHide:true}).trim();
  if(!/^[a-f0-9]{40}$/.test(sourceCommit))throw Error('RECEIPT_SOURCE_INVALID');
  let trackedChanges=false;
  try {execFileSync('git',['diff','--quiet','HEAD','--'],{cwd:source,windowsHide:true});}
  catch(error){if(error.status!==1)throw Error('RECEIPT_SOURCE_INVALID');trackedChanges=true;}
  return {sourceCommit,trackedChanges};
}
async function fileReceipt(path,handles,{limit=8*1024**3,capture=false,limitError='RECEIPT_FILE_UNSAFE'}={}) {
  const before=await lstat(path,{bigint:true});
  if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1n)throw Error('RECEIPT_FILE_UNSAFE');
  if(before.size>BigInt(limit))throw Error(limitError);
  const handle=await open(path,'r');
  // Keep the same handle until the complete inventory has been checked. Parsing
  // uses exactly the bytes hashed here, never a second pathname read.
  handles.push({handle,path,before});
  if(!same(before,await handle.stat({bigint:true})))throw Error('RECEIPT_FILE_CHANGED');
  const hash=createHash('sha256'),buffer=Buffer.alloc(64*1024),chunks=[];
  let bytes=0;
  for(;;){
    const read=await handle.read(buffer,0,buffer.length,bytes);
    if(!read.bytesRead)break;
    bytes+=read.bytesRead;
    if(bytes>limit||BigInt(bytes)>before.size)throw Error('RECEIPT_FILE_CHANGED');
    const chunk=buffer.subarray(0,read.bytesRead);hash.update(chunk);
    if(capture)chunks.push(Buffer.from(chunk));
  }
  if(BigInt(bytes)!==before.size)throw Error('RECEIPT_FILE_CHANGED');
  return {receipt:{bytes,sha256:hash.digest('hex')},content:capture?Buffer.concat(chunks):null};
}
export async function collectReceipt({binary,bundle,source=resolve(fileURLToPath(new URL('../',import.meta.url)))}) {
  const sourceBefore=sourceReceipt(source),handles=[];
  try {
    const root=resolve(bundle),directory=await lstat(root,{bigint:true});
    if(!directory.isDirectory()||directory.isSymbolicLink())throw Error('RECEIPT_BUNDLE_UNSAFE');
    const {receipt:binaryReceipt}=await fileReceipt(resolve(binary),handles);
    const {receipt:manifestReceipt,content}=await fileReceipt(join(root,'manifest.json'),handles,{limit:1024*1024,capture:true,limitError:'RECEIPT_MANIFEST_TOO_LARGE'});
    const manifest=JSON.parse(content.toString('utf8'));
    if(!Array.isArray(manifest.images)||manifest.images.length<1||manifest.images.length>32)throw Error('RECEIPT_MANIFEST_INVALID');
    const archives=[],names=new Set();
    for(const image of manifest.images){
      if(!image||typeof image!=='object')throw Error('RECEIPT_MANIFEST_INVALID');
      if(!image.archive)continue;
      const {path,bytes,sha256}=image.archive;
      // Public Runtime format uses one plain archive filename. Never follow arbitrary manifest paths.
      if(typeof path!=='string'||!/^[a-zA-Z0-9][a-zA-Z0-9._-]*\.tar$/.test(path)||!Number.isSafeInteger(bytes)||bytes<=0||bytes>8*1024**3||typeof sha256!=='string'||!/^[a-f0-9]{64}$/.test(sha256)||names.has(path.toLowerCase()))throw Error('RECEIPT_ARCHIVE_INVALID');
      names.add(path.toLowerCase());
      const {receipt:actual}=await fileReceipt(join(root,path),handles,{limit:bytes,limitError:'RECEIPT_ARCHIVE_MISMATCH'});
      if(actual.bytes!==bytes||actual.sha256!==sha256)throw Error('RECEIPT_ARCHIVE_MISMATCH');
      archives.push({name:path,...actual});
    }
    if(!archives.length)throw Error('RECEIPT_ARCHIVE_MISSING');
    for(const {handle,path,before} of handles){
      const current=await lstat(path,{bigint:true});
      if(!same(before,current)||!same(before,await handle.stat({bigint:true}))||!current.isFile()||current.nlink!==1n)throw Error('RECEIPT_FILE_CHANGED');
    }
    if(!same(directory,await lstat(root,{bigint:true})))throw Error('RECEIPT_BUNDLE_CHANGED');
    if(JSON.stringify(sourceBefore)!==JSON.stringify(sourceReceipt(source)))throw Error('RECEIPT_SOURCE_CHANGED');
    return {format:1,observedAt:new Date().toISOString(),platform:process.platform,architecture:process.arch,node:process.version,...sourceBefore,binary:binaryReceipt,manifest:manifestReceipt,archives,scope:'Read-only byte inventory; does not bind binary to source, authorize installation, verify signatures or prove Runtime readiness.'};
  } finally {
    await Promise.allSettled(handles.map(({handle})=>handle.close()));
  }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  try{
    const args=process.argv.slice(2);
    if(args.length!==4||args[0]!=='--binary'||args[2]!=='--bundle')throw Error('RECEIPT_USAGE');
    console.log(JSON.stringify(await collectReceipt({binary:args[1],bundle:args[3]}),null,2));
  }catch(error){console.error(/^RECEIPT_[A-Z_]+$/.test(error.message)?error.message:'RECEIPT_FAILED');process.exitCode=1;}
}
