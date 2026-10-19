// SPDX-License-Identifier: Apache-2.0
// Fixed helper entry. This file is hash-pinned by the native resource manifest.
import {open,lstat,unlink,readdir,realpath,statfs} from 'node:fs/promises';
import {constants} from 'node:fs';
import {createHash} from 'node:crypto';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const fail=()=>{throw Error('HOSTED_REMOTE_RESTORE_FAILED');};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function readFile(path,max){const before=await lstat(path);if(!before.isFile()||before.isSymbolicLink()||before.nlink!==1||before.size<1||before.size>max)fail();const f=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);try{const opened=await f.stat();if(opened.dev!==before.dev||opened.ino!==before.ino)fail();const bytes=await f.readFile();const after=await f.stat(),named=await lstat(path);if(bytes.length!==before.size||after.size!==before.size||after.mtimeMs!==before.mtimeMs||named.ino!==before.ino||named.dev!==before.dev)fail();return bytes;}finally{await f.close();}}
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
async function databaseIdentity(url,dependency){
 let row;if(dependency)row=await dependency(url);else{const {Pool}=(await import('pg')).default;const pool=new Pool({connectionString:url,connectionTimeoutMillis:30000,query_timeout:30000});try{row=(await pool.query("SELECT system_identifier::text AS system, (SELECT oid::text FROM pg_database WHERE datname=current_database()) AS oid, current_database() AS database FROM pg_control_system()")).rows[0];}finally{await pool.end();}}
 if(!row||!/^\d+$/.test(row.system)||!/^\d+$/.test(row.oid)||typeof row.database!=='string')fail();
 return hash(JSON.stringify({system:row.system,oid:row.oid,database:row.database}));
}
export async function verifyTargetIdentity(input,dependencies={}){
 if(!input||Object.keys(input).sort().join(',')!=='action,databaseURL,expectedIdentitySha256'||input.action!=='identity'||!/^[a-f0-9]{64}$/.test(input.expectedIdentitySha256))fail();
 let u;try{u=new URL(input.databaseURL);}catch{fail();}if(!['postgres:','postgresql:'].includes(u.protocol)||!u.username||!u.password||u.hash||u.port&&u.port!=='5432'||!/^\/[a-zA-Z0-9_-]+$/.test(u.pathname)||!/^[a-z0-9-]+\.[a-z0-9-]+-postgres\.render\.com$/.test(u.hostname)||u.searchParams.size!==1||u.searchParams.get('sslmode')!=='verify-full')fail();
 try{const actual=await databaseIdentity(u.href,dependencies.identity);if(actual!==input.expectedIdentitySha256)fail();return {operation:'identity-verified',targetIdentitySha256:actual};}finally{input.databaseURL='';}
}
// The only writable data root is this operation's fresh owned volume. Abort
// this helper itself on a breached native budget; never signal foreign work.
async function usage(root){let bytes=0,count=0;const visit=async dir=>{for(const entry of await readdir(dir,{withFileTypes:true})){if(++count>100000)fail();const path=join(dir,entry.name);let s;try{s=await lstat(path);}catch(error){if(error.code==='ENOENT')continue;throw error;}if(s.isSymbolicLink())fail();if(s.isDirectory())await visit(path);else if(s.isFile()){bytes+=s.size;if(!Number.isSafeInteger(bytes))fail();}else fail();}};await visit(root);return bytes;}
export async function restoreTarget(input,dependencies={}){
 if(!input||Object.keys(input).sort().join(',')!=='authenticatedManifestSha256,backupId,databaseURL,expectedAuthority,files,jobBudgetBytes,keyBase64,operationId'||!Number.isSafeInteger(input.jobBudgetBytes)||input.jobBudgetBytes<4194304||input.jobBudgetBytes>32*1024**3||!uuid(input.operationId)||!uuid(input.backupId)||!Number.isSafeInteger(input.files)||input.files<1||input.files>100000||!/^[a-f0-9]{64}$/.test(input.authenticatedManifestSha256))fail();
 const {runRemoteBackup,validatePublicAuthority}=dependencies.worker??await import('./remote-backup-worker.mjs');
 const expected=validatePublicAuthority(input.expectedAuthority);
 let connection;try{connection=new URL(input.databaseURL);}catch{fail();}
 if(!['postgres:','postgresql:'].includes(connection.protocol)||!connection.username||!connection.password||!/^\/[a-zA-Z0-9_-]+$/.test(connection.pathname)||!/^[a-z0-9-]+\.[a-z0-9-]+-postgres\.render\.com$/.test(connection.hostname)||connection.hash||connection.port&&connection.port!=='5432'||connection.searchParams.size!==1||connection.searchParams.get('sslmode')!=='verify-full')fail();
 const paths=dependencies.paths??{work:'/work',source:'/source',keys:'/run/exhibitos'};
 const key=Buffer.from(input.keyBase64,'base64');if(key.length!==32||key.toString('base64')!==input.keyBase64){key.fill(0);fail();}
 let keyFile,pin,monitor,monitoring=false,breached=false;
 const check=async()=>{if(await usage(paths.work)>input.jobBudgetBytes)fail();if(breached)fail();};
 try{
  for(const path of Object.values(paths)){const s=await lstat(path);if(!s.isDirectory()||s.isSymbolicLink()||await realpath(path)!==path)fail();}
  if((await readdir(paths.work)).length||(await readdir(paths.keys)).length)fail();
  const capacity=await statfs(paths.work,{bigint:true});if(capacity.bavail*capacity.bsize<BigInt(input.jobBudgetBytes))fail();
  monitor=setInterval(()=>{if(monitoring)return;monitoring=true;void check().catch(()=>{breached=true;(dependencies.onBudgetExceeded??(()=>process.exit(1)))();}).finally(()=>{monitoring=false;});},1000);

  keyFile=paths.keys+'/restore.key';const f=await open(keyFile,constants.O_WRONLY|constants.O_CREAT|constants.O_EXCL|constants.O_NOFOLLOW,0o600);
  try{await f.writeFile(key);await f.sync();pin=await f.stat();}finally{await f.close();}
  const authenticated=paths.work+'/authenticated';
  const verified=await runRemoteBackup({operation:'verify',source:paths.source,destination:authenticated,keyFile});
  await check();
  const manifestBytes=await readFile(authenticated+'/manifest.json',16*1024*1024),manifest=JSON.parse(manifestBytes);
  if(manifestBytes.length>16*1024*1024||hash(manifestBytes)!==input.authenticatedManifestSha256||manifest.id!==input.backupId||verified?.operation!=='verified'||verified.files!==input.files)fail();
  const records=manifest.files?.filter(f=>f.role==='configuration'&&f.name==='hosted-public-authority.json');
  if(!Array.isArray(records)||records.length!==1)fail();const record=records[0];
  if(!/^files\/[0-9]{8}\.gcm$/.test(record.path)||!Number.isSafeInteger(record.bytes)||record.bytes<1||record.bytes>4096)fail();
  const authorityBytes=await readFile(authenticated+'/'+record.path.slice(6)+'.plain',4096);
  if(authorityBytes.length!==record.bytes||hash(authorityBytes)!==record.sha256)fail();
  const authority=validatePublicAuthority(JSON.parse(authorityBytes));
  if(['origin','keyId','publicKey'].some(k=>authority[k]!==expected[k]))fail();
  // No SQL until the exact authenticated original archive/authority was checked.
  // Existing core enforces actual physical target distinction, empty SQL/blob
  // state, full table/sequence/blob/migration inventory and signer verification.
  await check();
  const restored=await runRemoteBackup({operation:'restore',source:paths.source,destination:paths.work+'/restored',keyFile},{withIsolatedDestination:callback=>callback({url:connection.href,role:'verified-remote-restore-target',expectedAuthority:authority})});
  await check();if(restored?.operation!=='restored-and-verified'||hash(await readFile(paths.work+'/restored/manifest.json',16*1024*1024))!==input.authenticatedManifestSha256)fail();
  return {operation:'restored-and-verified',backupId:input.backupId,authenticatedManifestSha256:input.authenticatedManifestSha256,files:input.files,targetIdentitySha256:await databaseIdentity(connection.href,dependencies.identity)};
 }finally{
  clearInterval(monitor);key.fill(0);input.keyBase64='';input.databaseURL='';
  if(pin&&keyFile){const named=await lstat(keyFile).catch(()=>null);if(named&&!named.isSymbolicLink()&&named.dev===pin.dev&&named.ino===pin.ino)await unlink(keyFile);}
 }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){try{const chunks=[];let total=0;for await(const chunk of process.stdin){total+=chunk.length;if(total>32768)fail();chunks.push(chunk);}const bytes=Buffer.concat(chunks);let input;try{input=JSON.parse(bytes);}finally{bytes.fill(0);for(const c of chunks)c.fill(0);}process.stdout.write(JSON.stringify(await (input.action==='identity'?verifyTargetIdentity(input):restoreTarget(input))));}catch{process.stdout.write('{"errorCode":"HOSTED_REMOTE_RESTORE_FAILED"}');process.exitCode=1;}}
