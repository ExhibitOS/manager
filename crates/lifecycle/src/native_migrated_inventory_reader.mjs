// SPDX-License-Identifier: Apache-2.0
// Fixed native reader: no caller-provided health success or physical DB identity.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {realpath} from 'node:fs/promises';
import {FileBlobStore,collectServiceInventory,verifyMigratedInventory} from '/opt/exhibitos/packages/storage/dist/index.js';
const require=createRequire('/opt/exhibitos/package.json'),{Pool}=require('pg');
const [sha,expectedSystem,catalogText,catalogPin,sourceSchema,targetSchema,migrationsPin]=process.argv.slice(1);
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const hash=v=>createHash('sha256').update(v).digest('hex');
if(![sha,catalogPin,sourceSchema,targetSchema,migrationsPin].every(hex)||sourceSchema===targetSchema||typeof catalogText!=='string'||Buffer.byteLength(catalogText)>65536||hash(catalogText)!==catalogPin)throw Error('UPDATE_CATALOG_INVALID');
const catalog=JSON.parse(catalogText);
if(hash(JSON.stringify({migrations:catalog.migrations,schemaDigest:catalog.schemaDigest,schemaVersion:catalog.schemaVersion}))!==targetSchema||hash(JSON.stringify(catalog.migrations))!==migrationsPin)throw Error('UPDATE_CATALOG_INVALID');
let size=0,chunks=[];
for await(const c of process.stdin){size+=c.length;if(size>16*1024*1024)throw Error('UPDATE_INPUT_INVALID');chunks.push(c);}
const raw=Buffer.concat(chunks);
if(!/^[a-f0-9]{64}$/.test(sha??'')||createHash('sha256').update(raw).digest('hex')!==sha)throw Error('UPDATE_INPUT_INVALID');
if(process.env.BLOB_ROOT!=='/data/blobs'||await realpath('/data/blobs')!=='/data/blobs')throw Error('UPDATE_BLOB_ROOT_INVALID');
const pool=new Pool({connectionString:process.env.DATABASE_URL,connectionTimeoutMillis:15000,statement_timeout:60000});
try{
 const r=await pool.query('SELECT system_identifier::text AS identifier FROM pg_control_system()'),system=r.rows[0]?.identifier;
 if(!/^[1-9][0-9]{0,19}$/.test(system??''))throw Error('UPDATE_DATABASE_ID_INVALID');
 if(expectedSystem===undefined||system!==expectedSystem)throw Error('UPDATE_DATABASE_ID_MISMATCH');
 const blobs=new FileBlobStore('/data/blobs');
 const proof=await verifyMigratedInventory({pool,manifestBytes:raw,expectedManifestSha256:sha,snapshotSystemIdentifier:system,targetSchemaSha256:targetSchema,targetMigrations:catalog.migrations,snapshot:c=>collectServiceInventory(c,blobs,{migrationCatalog:catalog.migrations})});
 if(proof.operation!=='migrated-inventory-preserved'||proof.sourceSchemaSha256!==sourceSchema||proof.targetSchemaSha256!==targetSchema||proof.targetMigrationsSha256!==migrationsPin||proof.originalDataPreserved!==true||proof.currentInventoryVerified!==false||proof.configurationVerified!==false||proof.preflightVerified!==false||proof.updateExecuted!==false)throw Error('UPDATE_INVENTORY_UNVERIFIED');
 console.log(JSON.stringify(proof));
}catch(e){const code=/^[A-Z][A-Z0-9_]{0,79}$/.test(e?.message??'')?e.message:'UPDATE_ROLLBACK_HEALTH_FAILED';process.stderr.write(code+'\n');process.exitCode=1;}
finally{await pool.end();}
