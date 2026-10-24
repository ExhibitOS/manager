// SPDX-License-Identifier: Apache-2.0
// Fixed 015 -> 016 application/reconciliation for an authenticated, fenced,
// unactivated candidate. Intent files and prior result hashes are not authority.
import {readFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
import {inspectionRequest,privateManifest} from './native-legacy-inspect.mjs';

const lastName='016_local_workspaces.sql';
const lastHash='cef351fc481576cac99f8b9653e6343b505b38f079d0f0789bf00367d2d69fab';
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const fail=()=>{throw Error('NATIVE_LEGACY_SCHEMA_FAILED');};
const comparable=(value,canonical)=>{const {createdAt,...rest}=value;return canonical(rest);};
const schema=(value,canonical)=>hash(canonical({schemaDigest:value.schemaDigest,schemaVersion:value.schemaVersion,migrations:value.migrations}));

export function schemaRequest(value){
 if(!value||typeof value!=='object'||Array.isArray(value)||!['apply','reconcile'].includes(value.operation))fail();
 const {operation,...binding}=value;
 return {...inspectionRequest(binding),operation};
}

// A caller-owned, single-client transaction. Reconciliation never commits even
// the temporary DROP/DELETE. The pool-owning verifiers run only after release.
export async function deriveFixedTarget(client,{manifest,catalog,directory,store,collect,migrate,canonical,systemIdentifier}){
 let transaction=false,broken=false;
 try{
  await client.query('BEGIN');transaction=true;
  if((await client.query('SELECT pg_try_advisory_xact_lock(82002) AS locked')).rows[0]?.locked!==true)fail();
  await client.query('SELECT pg_advisory_xact_lock(82001)');
  const identity=(await client.query('SELECT system_identifier::text AS system,current_database() AS database FROM pg_control_system()')).rows[0];
  if(identity?.system!==systemIdentifier||identity.database!==manifest.sourceIdentity.database||systemIdentifier===manifest.sourceIdentity.systemIdentifier)fail();
  const rows=(await client.query('SELECT name,sha256 FROM public.schema_migrations ORDER BY name')).rows;
  const applied=canonical(rows)===canonical(catalog);
  if(!applied&&canonical(rows)!==canonical(catalog.slice(0,15)))fail();
  let observed;
  if(applied){
   await client.query('LOCK TABLE public.local_workspaces IN ACCESS EXCLUSIVE MODE');
   if((await client.query('SELECT NOT EXISTS(SELECT 1 FROM public.local_workspaces) AS empty')).rows[0]?.empty!==true)fail();
   observed=await collect(client,store,{migrationDirectory:directory});
   if(observed.issues.length)fail();
   // Fixed additive table only; dependent/unknown objects make DROP fail.
   await client.query('DROP TABLE public.local_workspaces');
   const removed=await client.query('DELETE FROM public.schema_migrations WHERE name=$1 AND sha256=$2 RETURNING name',[lastName,lastHash]);
   if(removed.rowCount!==1)fail();
  }
  const original=await collect(client,store,{migrationCatalog:catalog.slice(0,15)});
  // Full authenticated inventory, including schema_migrations applied_at,
  // sequences, objects, references and original rows. Only createdAt varies.
  if(original.issues.length||comparable(original,canonical)!==comparable(manifest.inventory,canonical))fail();
  await migrate(client,directory);
  const target=await collect(client,store,{migrationDirectory:directory});
  if(target.issues.length||canonical(target.migrations)!==canonical(catalog)||
     (observed&&schema(observed,canonical)!==schema(target,canonical)))fail();
  return {applied,targetSchemaSha256:schema(target,canonical)};
 }finally{
  if(transaction)await client.query('ROLLBACK').catch(()=>{broken=true;});
  client.release(broken);
  if(broken)fail();
 }
}

async function run(request){
 const runtime=new URL('../../native-runtime/',import.meta.url);
 if(hash(await readFile(new URL('manifest.json',runtime)))!==request.payloadSha256)fail();
 const manifestBytes=await privateManifest(request.manifestPath);
 if(hash(manifestBytes)!==request.manifestSha256)fail();
 const manifest=boundedJSON(manifestBytes.toString('utf8'),16*1024*1024);
 if(manifest.id!==request.backupId)fail();
 const database=new URL(process.env.DATABASE_URL??'');
 if(!['postgres:','postgresql:'].includes(database.protocol)||database.hostname!=='127.0.0.1'||database.pathname!=='/exhibitos'||database.search||database.hash||!database.port||!database.username||!database.password)fail();
 const blobRoot=process.env.BLOB_ROOT;
 if(!blobRoot||resolve(blobRoot)!==blobRoot)fail();
 const {realpath}=await import('node:fs/promises');
 if(await realpath(blobRoot)!==blobRoot)fail();
 const {default:pg}=await import(new URL('platform/node_modules/pg/lib/index.js',runtime));
 const {FileBlobStore}=await import(new URL('platform/packages/storage/dist/blobs.js',runtime));
 const {collectServiceInventory,inventoryCanonical}=await import(new URL('platform/packages/storage/dist/service-inventory.js',runtime));
 const {verifyRestoredInventory,verifyMigratedInventory}=await import(new URL('platform/packages/storage/dist/service-backup.js',runtime));
 const {migrateInTransaction}=await import('../vendor/platform/migrations.mjs');
 const directory=fileURLToPath(new URL('platform/database/migrations/',runtime)),catalog=[];
 for(const name of (await readdir(directory)).filter(n=>/^\d+.*\.sql$/.test(n)).sort())catalog.push({name,sha256:hash(await readFile(resolve(directory,name)))});
 if(catalog.length!==16||catalog[15].name!==lastName||catalog[15].sha256!==lastHash||
    !Array.isArray(manifest.inventory?.migrations)||manifest.inventory.migrations.length!==15||
    inventoryCanonical(manifest.inventory.migrations)!==inventoryCanonical(catalog.slice(0,15)))fail();
 const pool=new pg.Pool({connectionString:database.href,max:1,connectionTimeoutMillis:5000,statement_timeout:30000});
 const store=new FileBlobStore(blobRoot),options={pool,manifestBytes,expectedManifestSha256:request.manifestSha256,snapshotSystemIdentifier:request.systemIdentifier,snapshot:(client,migrations)=>collectServiceInventory(client,store,migrations?{migrationCatalog:migrations}:{migrationDirectory:directory})};
 const context={manifest,catalog,directory,store,collect:collectServiceInventory,migrate:migrateInTransaction,canonical:inventoryCanonical,systemIdentifier:request.systemIdentifier};
 try{
  const derived=await deriveFixedTarget(await pool.connect(),context);
  if(!derived.applied){
   // Re-prove original state outside the single-client transaction. A recovery
   // request NEVER replays an uncommitted application automatically.
   await verifyRestoredInventory(options);
   if(request.operation==='apply'){
    const client=await pool.connect();let transaction=false,broken=false;
    try{
     await client.query('BEGIN');transaction=true;
     if((await client.query('SELECT pg_try_advisory_xact_lock(82002) AS locked')).rows[0]?.locked!==true)fail();
     await client.query('SELECT pg_advisory_xact_lock(82001)');
     const identity=(await client.query('SELECT system_identifier::text AS system,current_database() AS database FROM pg_control_system()')).rows[0];
     if(identity?.system!==request.systemIdentifier||identity.database!==manifest.sourceIdentity.database)fail();
     const original=await collectServiceInventory(client,store,{migrationCatalog:catalog.slice(0,15)});
     if(original.issues.length||comparable(original,inventoryCanonical)!==comparable(manifest.inventory,inventoryCanonical))fail();
     await migrateInTransaction(client,directory);
     const target=await collectServiceInventory(client,store,{migrationDirectory:directory});
     if(target.issues.length||schema(target,inventoryCanonical)!==derived.targetSchemaSha256)fail();
     // If COMMIT reports an error its outcome remains unknown. Rust retains
     // the marker/intent and a fresh authenticated reconcile must observe it.
     await client.query('COMMIT');transaction=false;
    }finally{
     if(transaction)await client.query('ROLLBACK').catch(()=>{broken=true;});
     client.release(broken);if(broken)fail();
    }
   }
  }else if(request.operation==='apply')fail();
  const applied=derived.applied||request.operation==='apply';
  if(applied)await verifyMigratedInventory({...options,targetSchemaSha256:derived.targetSchemaSha256,targetMigrations:catalog});
  return {format:1,operation:applied?'legacy-schema-applied':'legacy-schema-unapplied',candidateId:request.candidateId,backupId:request.backupId,manifestSha256:request.manifestSha256,payloadSha256:request.payloadSha256,systemIdentifier:request.systemIdentifier,sourceInventorySha256:hash(inventoryCanonical(manifest.inventory)),sourceSchemaSha256:schema(manifest.inventory,inventoryCanonical),targetSchemaSha256:derived.targetSchemaSha256,targetMigrations:catalog,originalDataPreserved:true};
 }finally{await pool.end();}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const chunks=[];let bytes=0;
  for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>16384)fail();chunks.push(chunk);}
  const request=schemaRequest(boundedJSON(Buffer.concat(chunks).toString('utf8'),16384));
  const result=JSON.stringify(await run(request));if(Buffer.byteLength(result)>16384)fail();process.stdout.write(result);
 }catch{process.stdout.write(JSON.stringify({errorCode:'NATIVE_LEGACY_SCHEMA_FAILED'}));process.exitCode=1;}
}
