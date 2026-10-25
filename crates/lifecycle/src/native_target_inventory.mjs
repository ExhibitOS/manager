// SPDX-License-Identifier: Apache-2.0
// Fixed read-only native candidate observer. No migration, restore, bootstrap,
// arbitrary module/URL input or application writer starts here.
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {readdir,realpath} from 'node:fs/promises';
import {createReadStream,constants} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
const [platformRoot,manifestSHA,targetSchema,expectedSystem,migrationText,migrationPin]=process.argv.slice(1);
const hash=v=>createHash('sha256').update(v).digest('hex');
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
let pool,manifestBytes;
try {
 if(![manifestSHA,targetSchema,migrationPin].every(hex)||typeof migrationText!=='string'||Buffer.byteLength(migrationText)>65536||hash(migrationText)!==migrationPin||await realpath(platformRoot)!==platformRoot)throw Error('NATIVE_TARGET_INPUT_INVALID');
 const migrations=JSON.parse(migrationText),directory=join(platformRoot,'database/migrations');
 if(!Array.isArray(migrations)||!migrations.length||migrations.length>10000||migrations.some((m,i)=>!m||Object.keys(m).sort().join(',')!=='name,sha256'||typeof m.name!=='string'||m.name.length>256||!/^[0-9][a-zA-Z0-9_.-]*\.sql$/.test(m.name)||!hex(m.sha256)||(i&&migrations[i-1].name>=m.name)))throw Error('NATIVE_TARGET_INPUT_INVALID');
 async function checkMigrations(){
  const names=(await readdir(directory)).filter(n=>/^[0-9].*\.sql$/.test(n)).sort();
  if(JSON.stringify(names)!==JSON.stringify(migrations.map(m=>m.name)))throw Error('NATIVE_TARGET_PAYLOAD_CHANGED');
  for(const m of migrations){
   const p=join(directory,m.name);if(await realpath(p)!==p)throw Error('NATIVE_TARGET_PAYLOAD_CHANGED');
   let size=0;const digest=createHash('sha256');
   for await(const chunk of createReadStream(p,{flags:constants.O_RDONLY|constants.O_NOFOLLOW,highWaterMark:65536})){size+=chunk.length;if(size>2*1024*1024)throw Error('NATIVE_TARGET_PAYLOAD_CHANGED');digest.update(chunk);}
   if(await realpath(p)!==p||digest.digest('hex')!==m.sha256)throw Error('NATIVE_TARGET_PAYLOAD_CHANGED');
  }
 }
 await checkMigrations();
 let size=0,chunks=[];
 for await(const chunk of process.stdin){size+=chunk.length;if(size>16*1024*1024)throw Error('NATIVE_TARGET_INPUT_INVALID');chunks.push(chunk);}
 manifestBytes=Buffer.concat(chunks);chunks=[];
 if(!manifestBytes.length||hash(manifestBytes)!==manifestSHA)throw Error('NATIVE_TARGET_INPUT_INVALID');
 const blobRoot=process.env.BLOB_ROOT;
 if(typeof blobRoot!=='string'||await realpath(blobRoot)!==blobRoot)throw Error('NATIVE_TARGET_INPUT_INVALID');
 const require=createRequire(pathToFileURL(join(platformRoot,'package.json'))),{Pool}=require('pg');
 const {FileBlobStore,collectServiceInventory,verifyRestoredInventory,verifyMigratedInventory}=await import(pathToFileURL(join(platformRoot,'packages/storage/dist/index.js')).href);
 if([FileBlobStore,collectServiceInventory,verifyRestoredInventory,verifyMigratedInventory].some(v=>typeof v!=='function'))throw Error('NATIVE_TARGET_CODEC_UNAVAILABLE');
 pool=new Pool({connectionString:process.env.DATABASE_URL,connectionTimeoutMillis:15000,statement_timeout:60000});
 const id=async()=>{const r=await pool.query('SELECT system_identifier::text AS identifier FROM pg_control_system()');return r.rows[0]?.identifier;};
 const system=await id();
 if(!/^[1-9][0-9]{0,19}$/.test(system??'')||BigInt(system)>18446744073709551615n||system!==expectedSystem)throw Error('NATIVE_TARGET_DATABASE_CHANGED');
 const blobs=new FileBlobStore(blobRoot),manifest=JSON.parse(manifestBytes.toString('utf8'));
 const sourceMigrations=manifest.inventory?.migrations;
 const sameCatalog=JSON.stringify(sourceMigrations?.map(m=>({name:m.name,sha256:m.sha256})))===JSON.stringify(migrations);
 const options={pool,manifestBytes,expectedManifestSha256:manifestSHA,snapshotSystemIdentifier:system,snapshot:c=>collectServiceInventory(c,blobs,{migrationCatalog:migrations})};
 const proof=sameCatalog?await verifyRestoredInventory(options):await verifyMigratedInventory({...options,targetSchemaSha256:targetSchema,targetMigrations:migrations});
 if(sameCatalog?(proof.operation!=='restored-inventory-matched'||proof.schemaSha256!==targetSchema||proof.currentInventoryVerified!==true):(proof.operation!=='migrated-inventory-preserved'||proof.targetSchemaSha256!==targetSchema||proof.targetMigrationsSha256!==migrationPin||proof.originalDataPreserved!==true||proof.currentInventoryVerified!==false))throw Error('NATIVE_TARGET_INVENTORY_INVALID');
 if(proof.configurationVerified!==false||proof.preflightVerified!==false||proof.updateExecuted!==false||await id()!==system)throw Error('NATIVE_TARGET_INVENTORY_INVALID');
 await checkMigrations();
 console.log(JSON.stringify({operation:'native-target-inventory-observed',backupId:proof.backupId,authenticatedManifestSha256:proof.authenticatedManifestSha256,sourceInventorySha256:proof.sourceInventorySha256??proof.inventorySha256,sourceSchemaSha256:proof.sourceSchemaSha256??proof.schemaSha256,targetSchemaSha256:targetSchema,targetMigrationsSha256:migrationPin,targetSystemIdentifier:system,originalDataPreserved:true,configurationVerified:false,preflightVerified:false,updateExecuted:false}));
} catch {
 process.stderr.write('NATIVE_TARGET_OBSERVATION_FAILED\n');process.exitCode=1;
} finally {if(manifestBytes)manifestBytes.fill(0);if(pool)await pool.end();}
