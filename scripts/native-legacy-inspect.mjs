// SPDX-License-Identifier: Apache-2.0
// Fixed inspection entry for an authenticated, isolated legacy candidate.
// The native owner holds the candidate fence and admits every module/SQL byte.
// This entry never commits a migration or grants activation from a receipt.
import {constants} from 'node:fs';
import {open,lstat,realpath,readFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const fail=()=>{throw Error('NATIVE_LEGACY_INSPECTION_FAILED');};
export function inspectionRequest(value){
 if(!value||typeof value!=='object'||Array.isArray(value)||Object.keys(value).sort().join(',')!=='backupId,candidateId,manifestPath,manifestSha256,payloadSha256,systemIdentifier')fail();
 for(const field of ['backupId','candidateId'])if(typeof value[field]!=='string'||!/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value[field]))fail();
 if(typeof value.manifestPath!=='string'||resolve(value.manifestPath)!==value.manifestPath||!hex(value.manifestSha256)||!hex(value.payloadSha256)||typeof value.systemIdentifier!=='string'||!/^[1-9][0-9]{0,19}$/.test(value.systemIdentifier)||BigInt(value.systemIdentifier)>18446744073709551615n)fail();
 return {...value};
}
export async function privateManifest(path){
 if(await realpath(path)!==path)fail();
 const file=await open(path,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
 try{
  const before=await file.stat({bigint:true});
  if(!before.isFile()||before.nlink!==1n||(before.mode&0o7777n)!==0o600n||before.uid!==BigInt(process.getuid())||before.size<1n||before.size>16n*1024n*1024n)fail();
  const bytes=await file.readFile(),after=await file.stat({bigint:true}),named=await lstat(path,{bigint:true});
  if(await realpath(path)!==path||['dev','ino','size','mode','uid','nlink','mtimeNs','ctimeNs'].some(k=>before[k]!==after[k]||before[k]!==named[k]))fail();
  return bytes;
 }finally{await file.close();}
}
async function inspect(request){
 const runtime=new URL('../../native-runtime/',import.meta.url);
 if(hash(await readFile(new URL('manifest.json',runtime)))!==request.payloadSha256)fail();
 const manifestBytes=await privateManifest(request.manifestPath);
 if(hash(manifestBytes)!==request.manifestSha256)fail();
 const manifest=boundedJSON(manifestBytes.toString('utf8'),16*1024*1024);
 if(manifest.id!==request.backupId)fail();
 const database=new URL(process.env.DATABASE_URL??'');
 if(!['postgres:','postgresql:'].includes(database.protocol)||database.hostname!=='127.0.0.1'||database.pathname!=='/exhibitos'||database.search||database.hash||!database.port||!database.username||!database.password)fail();
 const blobRoot=process.env.BLOB_ROOT;
 if(!blobRoot||resolve(blobRoot)!==blobRoot||await realpath(blobRoot)!==blobRoot)fail();
 // Literal app siblings only; neither archive nor request selects executable code.
 const {default:pg}=await import(new URL('platform/node_modules/pg/lib/index.js',runtime));
 const {FileBlobStore}=await import(new URL('platform/packages/storage/dist/blobs.js',runtime));
 const {collectServiceInventory,inventoryCanonical}=await import(new URL('platform/packages/storage/dist/service-inventory.js',runtime));
 const {verifyRestoredInventory}=await import(new URL('platform/packages/storage/dist/service-backup.js',runtime));
 const {migrateInTransaction}=await import('../vendor/platform/migrations.mjs');
 const directory=fileURLToPath(new URL('platform/database/migrations/',runtime));
 const catalog=[];
 for(const name of (await readdir(directory)).filter(n=>/^\d+.*\.sql$/.test(n)).sort())catalog.push({name,sha256:hash(await readFile(resolve(directory,name)))});
 const original=manifest.inventory?.migrations;
 // This inspected contract is the existing additive 015 -> 016 transition only.
 // Future SQL needs an explicit side-effect review before rollback inspection.
 if(catalog.length!==16||catalog.at(-1)?.name!=='016_local_workspaces.sql'||!Array.isArray(original)||original.length!==15||inventoryCanonical(original)!==inventoryCanonical(catalog.slice(0,15)))fail();
 const pool=new pg.Pool({connectionString:database.href,max:1,connectionTimeoutMillis:5000,statement_timeout:30000});
 const store=new FileBlobStore(blobRoot),options={pool,manifestBytes,expectedManifestSha256:request.manifestSha256,snapshotSystemIdentifier:request.systemIdentifier,snapshot:(client,migrations)=>collectServiceInventory(client,store,{migrationCatalog:migrations})};
 let target;
 try{
  const before=await verifyRestoredInventory(options);
  const client=await pool.connect();let transaction=false,broken=false;
  try{
   await client.query('BEGIN');transaction=true;
   if((await client.query('SELECT pg_try_advisory_xact_lock(82002) AS locked')).rows[0]?.locked!==true)fail();
   const identity=(await client.query('SELECT system_identifier::text AS system,current_database() AS database FROM pg_control_system()')).rows[0];
   if(identity?.system!==request.systemIdentifier||identity.database!=='exhibitos')fail();
   await migrateInTransaction(client,directory);
   target=await collectServiceInventory(client,store,{migrationDirectory:directory});
   if(target.issues.length||inventoryCanonical(target.migrations)!==inventoryCanonical(catalog))fail();
   await client.query('ROLLBACK');transaction=false;
  }finally{
   if(transaction)await client.query('ROLLBACK').catch(()=>{broken=true;});
   client.release(broken);
   if(broken)fail();
  }
  const after=await verifyRestoredInventory(options);
  if(before.inventorySha256!==after.inventorySha256||before.schemaSha256!==after.schemaSha256)fail();
  const schema=value=>hash(inventoryCanonical({schemaDigest:value.schemaDigest,schemaVersion:value.schemaVersion,migrations:value.migrations}));
  if(!hex(target.schemaDigest)||schema(target)===before.schemaSha256)fail();
  return {format:1,operation:'legacy-schema-inspected-not-applied',candidateId:request.candidateId,backupId:request.backupId,manifestSha256:request.manifestSha256,payloadSha256:request.payloadSha256,systemIdentifier:request.systemIdentifier,sourceInventorySha256:before.inventorySha256,sourceSchemaSha256:before.schemaSha256,targetSchemaSha256:schema(target),targetMigrations:catalog,originalInventoryVerifiedAfterRollback:true};
 }finally{await pool.end();}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const chunks=[];let bytes=0;
  for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>16384)fail();chunks.push(chunk);}
  const request=inspectionRequest(boundedJSON(Buffer.concat(chunks).toString('utf8'),16384));
  const result=JSON.stringify(await inspect(request));if(Buffer.byteLength(result)>16384)fail();
  process.stdout.write(result);
 }catch{process.stdout.write(JSON.stringify({errorCode:'NATIVE_LEGACY_INSPECTION_FAILED'}));process.exitCode=1;}
}
