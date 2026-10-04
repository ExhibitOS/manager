// SPDX-License-Identifier: Apache-2.0
// Helper only: writable independent snapshot, original blobs/manifest read-only.
import {mkdir,chown,chmod} from 'node:fs/promises';
import {spawn,spawnSync} from 'node:child_process';
import {main} from '/opt/exhibitos/scripts/service-backup.mjs';
const socket='/tmp/exhibitos-source-pg',delay=ms=>new Promise(r=>setTimeout(r,ms));
let server,result,error,exit,finished=false;
try{
 await mkdir(socket,{mode:0o700});await chmod(socket,0o700);await chown(socket,999,999);
 server=spawn('/usr/lib/postgresql/18/bin/postgres',['-D','/snapshot/18/docker','-c','listen_addresses=','-c','unix_socket_directories='+socket,'-c','unix_socket_permissions=0700','-c','autovacuum=off','-c','default_transaction_read_only=on','-c','archive_mode=off','-c','shared_preload_libraries=','-c','session_preload_libraries=','-c','local_preload_libraries='],{uid:999,gid:999,stdio:'ignore'});
 exit=new Promise(resolve=>{server.once('error',()=>{finished=true;resolve(-1);});server.once('exit',code=>{finished=true;resolve(code);});});
 let ready=false;
 for(let i=0;i<100;i++){
  if(finished)throw Error('SOURCE_SNAPSHOT_DATABASE_START_FAILED');
  const r=spawnSync('/usr/lib/postgresql/18/bin/pg_isready',['-h',socket,'-U','exhibitos','-d','exhibitos'],{stdio:'ignore',timeout:1000});
  if(r.status===0){ready=true;break;}await delay(100);
 }
 if(!ready)throw Error('SOURCE_SNAPSHOT_DATABASE_START_FAILED');
 result=await main(['check-source-inventory','--manifest-file','/manifest.json','--manifest-sha256',process.env.EXHIBITOS_MANIFEST_SHA256,'--quiesced'],{DATABASE_URL:'postgresql://exhibitos@localhost/exhibitos?host='+encodeURIComponent(socket),BLOB_BACKEND:'file',BLOB_ROOT:'/blobs'});
 if(!result.currentInventoryVerified||result.preflightVerified||result.updateExecuted)throw Error('SOURCE_SNAPSHOT_DATABASE_PROOF_INVALID');
}catch(e){error=/^[A-Z][A-Z0-9_]{0,79}$/.test(e?.message??'')?e.message:'SOURCE_SNAPSHOT_DATABASE_FAILED';}
finally{
 if(server&&!finished){
  try{server.kill('SIGINT');const code=await Promise.race([exit,delay(20000).then(()=>null)]);if(code!==0){error='SOURCE_SNAPSHOT_DATABASE_STOP_FAILED';if(!finished){server.kill('SIGKILL');await exit;}}}
  catch{error='SOURCE_SNAPSHOT_DATABASE_STOP_FAILED';}
 }else if(server)error??='SOURCE_SNAPSHOT_DATABASE_STOP_FAILED';
}
if(error){console.error(error);process.exitCode=1;}else console.log(JSON.stringify(result));
