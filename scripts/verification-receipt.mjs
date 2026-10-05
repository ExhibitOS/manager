// SPDX-License-Identifier: Apache-2.0
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import {lstat,readFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve,join} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';

async function fileReceipt(path) {
  const before=await lstat(path);
  if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1||before.size>8*1024**3)throw Error('RECEIPT_FILE_UNSAFE');
  const hash=createHash('sha256');for await(const bytes of createReadStream(path))hash.update(bytes);
  const after=await lstat(path);
  if(before.dev!==after.dev||before.ino!==after.ino||before.size!==after.size||before.mtimeMs!==after.mtimeMs)throw Error('RECEIPT_FILE_CHANGED');
  return {bytes:before.size,sha256:hash.digest('hex')};
}
export async function collectReceipt({binary,bundle,source=resolve(fileURLToPath(new URL('../',import.meta.url)))}) {
  const sourceCommit=execFileSync('git',['rev-parse','HEAD'],{cwd:source,encoding:'utf8',windowsHide:true}).trim();
  if(!/^[a-f0-9]{40}$/.test(sourceCommit))throw Error('RECEIPT_SOURCE_INVALID');
  let trackedChanges=false;
  try {execFileSync('git',['diff','--quiet','HEAD','--'],{cwd:source,windowsHide:true});}
  catch(error){if(error.status!==1)throw Error('RECEIPT_SOURCE_INVALID');trackedChanges=true;}
  const binaryReceipt=await fileReceipt(resolve(binary));
  const root=resolve(bundle),directory=await lstat(root);
  if(!directory.isDirectory()||directory.isSymbolicLink())throw Error('RECEIPT_BUNDLE_UNSAFE');
  const manifestReceipt=await fileReceipt(join(root,'manifest.json'));
  if(manifestReceipt.bytes>1024*1024)throw Error('RECEIPT_MANIFEST_TOO_LARGE');
  const manifest=JSON.parse(await readFile(join(root,'manifest.json'),'utf8'));
  if(!Array.isArray(manifest.images)||manifest.images.length<1||manifest.images.length>32)throw Error('RECEIPT_MANIFEST_INVALID');
  const archives=[];
  for(const image of manifest.images){
    if(!image.archive)continue;
    const {path,bytes,sha256}=image.archive;
    // Public Runtime format uses one plain archive filename. Never follow arbitrary manifest paths.
    if(typeof path!=='string'||!/^[a-zA-Z0-9][a-zA-Z0-9._-]*\.tar$/.test(path)||!Number.isSafeInteger(bytes)||bytes<=0||typeof sha256!=='string'||!/^[a-f0-9]{64}$/.test(sha256))throw Error('RECEIPT_ARCHIVE_INVALID');
    const actual=await fileReceipt(join(root,path));
    if(actual.bytes!==bytes||actual.sha256!==sha256)throw Error('RECEIPT_ARCHIVE_MISMATCH');
    archives.push({name:path,...actual});
  }
  if(!archives.length)throw Error('RECEIPT_ARCHIVE_MISSING');
  return {format:1,observedAt:new Date().toISOString(),platform:process.platform,architecture:process.arch,node:process.version,sourceCommit,trackedChanges,binary:binaryReceipt,manifest:manifestReceipt,archives,scope:'Read-only byte inventory; does not bind binary to source, authorize installation, verify signatures or prove Runtime readiness.'};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
  try{
    const args=process.argv.slice(2);
    if(args.length!==4||args[0]!=='--binary'||args[2]!=='--bundle')throw Error('RECEIPT_USAGE');
    console.log(JSON.stringify(await collectReceipt({binary:args[1],bundle:args[3]}),null,2));
  }catch(error){console.error(/^RECEIPT_[A-Z_]+$/.test(error.message)?error.message:'RECEIPT_FAILED');process.exitCode=1;}
}
