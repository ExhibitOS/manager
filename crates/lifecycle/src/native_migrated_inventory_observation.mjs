// SPDX-License-Identifier: Apache-2.0
// Compiled-in shared observer; executed only in the qualified maintenance image.
async function observeNativeMigratedInventory({pool,manifestBytes,manifestSha256,expectedSystem,catalog,sourceSchema,targetSchema,migrationsPin,blobRoot}) {
 const {createHash}=await import('node:crypto');
 const {realpath}=await import('node:fs/promises');
 const {FileBlobStore,collectServiceInventory,verifyMigratedInventory}=await import('/opt/exhibitos/packages/storage/dist/index.js');
 const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
 const hash=v=>createHash('sha256').update(v).digest('hex');
 if(![manifestSha256,sourceSchema,targetSchema,migrationsPin].every(hex)||sourceSchema===targetSchema||!catalog||hash(JSON.stringify({migrations:catalog.migrations,schemaDigest:catalog.schemaDigest,schemaVersion:catalog.schemaVersion}))!==targetSchema||hash(JSON.stringify(catalog.migrations))!==migrationsPin)throw Error('UPDATE_CATALOG_INVALID');
 if(!Buffer.isBuffer(manifestBytes)||manifestBytes.length>16*1024*1024||hash(manifestBytes)!==manifestSha256)throw Error('UPDATE_INPUT_INVALID');
 if(!['/data/blobs','/blobs'].includes(blobRoot)||await realpath(blobRoot)!==blobRoot)throw Error('UPDATE_BLOB_ROOT_INVALID');
 const r=await pool.query('SELECT system_identifier::text AS identifier FROM pg_control_system()'),system=r.rows[0]?.identifier;
 if(!/^[1-9][0-9]{0,19}$/.test(system??''))throw Error('UPDATE_DATABASE_ID_INVALID');
 if(system!==expectedSystem)throw Error('UPDATE_DATABASE_ID_MISMATCH');
 const blobs=new FileBlobStore(blobRoot);
 const proof=await verifyMigratedInventory({pool,manifestBytes,expectedManifestSha256:manifestSha256,snapshotSystemIdentifier:system,targetSchemaSha256:targetSchema,targetMigrations:catalog.migrations,snapshot:c=>collectServiceInventory(c,blobs,{migrationCatalog:catalog.migrations})});
 if(proof.operation!=='migrated-inventory-preserved'||proof.sourceSchemaSha256!==sourceSchema||proof.targetSchemaSha256!==targetSchema||proof.targetMigrationsSha256!==migrationsPin||proof.originalDataPreserved!==true||proof.currentInventoryVerified!==false||proof.configurationVerified!==false||proof.preflightVerified!==false||proof.updateExecuted!==false)throw Error('UPDATE_INVENTORY_UNVERIFIED');
 return proof;
}
