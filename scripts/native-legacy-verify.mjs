// SPDX-License-Identifier: Apache-2.0
// Fixed codec-only entry: no database, selected executable, network or key path.
import {fstatSync,readSync,openSync,closeSync,constants} from 'node:fs';
import {realpath,statfs} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const names=new Set(['manager-bundle-manifest.json','manager-compose.yaml','manager-runtime.env','manager-installed.json','manager-engine.json','manager-image-inventory.json','freeze-signing-key.json']);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const fail=code=>{throw Error(code);};
function readHeldPrivate(fd,max,exact,errorCode){
 const before=fstatSync(fd,{bigint:true});
 if(!before.isFile()||before.size<1n||before.size>BigInt(max)||(exact!==undefined&&before.size!==BigInt(exact))||before.nlink!==1n||(before.mode&0o7777n)!==0o600n||before.uid!==BigInt(process.getuid()))fail(errorCode);
 const key=Buffer.alloc(Number(before.size));
 try{
  let offset=0;while(offset<key.length){const n=readSync(fd,key,offset,key.length-offset,offset);if(n<1)fail(errorCode);offset+=n;}
  const after=fstatSync(fd,{bigint:true});
  if(['dev','ino','size','mode','uid','nlink','mtimeNs','ctimeNs'].some(k=>before[k]!==after[k]))fail(errorCode);
  return key;
 }catch(error){key.fill(0);throw error;}
}
export function readHeldKey(fd=3){return readHeldPrivate(fd,32,32,'NATIVE_LEGACY_KEY_HANDLE_INVALID');}
export function closedResult(verified,manifestBytes,expectedCipherManifest){
 const manifest=boundedJSON(manifestBytes.toString('utf8'),16*1024*1024);
 if(manifest.id!==verified.receipt?.id||verified.receipt?.status!=='complete'||verified.receipt.manifestSha256!==expectedCipherManifest||!Array.isArray(verified.files)||verified.files.length!==manifest.files?.length)fail('NATIVE_LEGACY_RESULT_INVALID');
 const configuration={};
 for(const file of verified.files.filter(f=>f.role==='configuration')){
  if(!names.has(file.name)||Object.hasOwn(configuration,file.name)||!Number.isSafeInteger(file.bytes)||file.bytes<1||file.bytes>1024*1024||!/^[a-f0-9]{64}$/.test(file.sha256))fail('NATIVE_LEGACY_RESULT_INVALID');
  configuration[file.name]={bytes:file.bytes,sha256:file.sha256};
 }
 if(Object.keys(configuration).length!==names.size)fail('NATIVE_LEGACY_RESULT_INVALID');
 return {format:1,operation:'legacy-configuration-authenticated',backupId:manifest.id,files:verified.files.length,authenticatedManifestSha256:hash(manifestBytes),archiveManifestSha256:expectedCipherManifest,configuration};
}
export function safeFailure(error){
 return ['NATIVE_LEGACY_KEY_HANDLE_INVALID','NATIVE_LEGACY_INPUT_INVALID','NATIVE_LEGACY_RESULT_INVALID','NATIVE_LEGACY_RESOURCE_BUDGET'].includes(error?.message)?error.message:'NATIVE_LEGACY_AUTHENTICATION_FAILED';
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 let key;
 try{
  const chunks=[];let count=0;for await(const chunk of process.stdin){count+=chunk.length;if(count>16384)fail('NATIVE_LEGACY_INPUT_INVALID');chunks.push(chunk);}
  const request=boundedJSON(Buffer.concat(chunks).toString('utf8'),16384);
  if(!request||Object.keys(request).sort().join(',')!=='archiveManifestSha256,source,workspace,writeBudgetBytes'||typeof request.source!=='string'||typeof request.workspace!=='string'||!/^[a-f0-9]{64}$/.test(request.archiveManifestSha256)||!Number.isSafeInteger(request.writeBudgetBytes)||request.writeBudgetBytes<1)fail('NATIVE_LEGACY_INPUT_INVALID');
  if(await realpath(request.source)!==request.source||await realpath(request.workspace)!==request.workspace)fail('NATIVE_LEGACY_INPUT_INVALID');
  const space=await statfs(request.workspace,{bigint:true});
  if(space.bavail*space.bsize<BigInt(request.writeBudgetBytes)+8n*1024n**3n)fail('NATIVE_LEGACY_RESOURCE_BUDGET');
  key=readHeldKey();
  // Literal app-sibling URL; never URL.pathname or a module from the archive.
  // This exact existing module imports fs/crypto/stream helpers, with pg types erased.
  const {verifyServiceBackup}=await import(new URL('../../native-runtime/platform/packages/storage/dist/service-backup.js',import.meta.url));
  const destination=resolve(request.workspace,'verified');
  const verified=await verifyServiceBackup({source:request.source,destination,encryptionKey:key});
  const fd=openSync(resolve(destination,'manifest.json'),constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
  let manifestBytes;try{manifestBytes=readHeldPrivate(fd,16*1024*1024,undefined,'NATIVE_LEGACY_RESULT_INVALID');}finally{closeSync(fd);}
  const result=JSON.stringify(closedResult(verified,manifestBytes,request.archiveManifestSha256));
  if(Buffer.byteLength(result)>16384)fail('NATIVE_LEGACY_RESULT_INVALID');
  process.stdout.write(result);
 }catch(error){process.stdout.write(JSON.stringify({errorCode:safeFailure(error)}));process.exitCode=1;}
 finally{key?.fill(0);}
}
