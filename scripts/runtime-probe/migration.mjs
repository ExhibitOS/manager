// SPDX-License-Identifier: Apache-2.0
// Native host supplies signed-plan and retained catalog identities as MIGRATION_INPUT.
const migrated = typeof MIGRATION_INPUT !== 'undefined';
let migrationState;
async function originalMigrationState(environment, ready) {
  if (!migrated) return;
  const response = await fetch('http://127.0.0.1:5433/manifest', {signal:AbortSignal.timeout(5000)});
  if (!response.ok) throw Error('RUNTIME_PROBE_MANIFEST_INVALID');
  const parts=[];let size=0;for await(const b of response.body){size+=b.length;if(size>16*1024*1024)throw Error('RUNTIME_PROBE_MANIFEST_INVALID');parts.push(b);}
  const bytes=Buffer.concat(parts);
  if(createHash('sha256').update(bytes).digest('hex')!==MIGRATION_INPUT.manifestSha256)throw Error('RUNTIME_PROBE_MANIFEST_INVALID');
  const manifest=JSON.parse(bytes.toString('utf8'));
  const directory='/probe/original-migrations';await mkdir(directory,{mode:0o700});
  for(const row of manifest.inventory.migrations){
    if(!/^[0-9][a-zA-Z0-9_.-]*\.sql$/.test(row.name)||row.name.length>1024||!/^[a-f0-9]{64}$/.test(row.sha256))throw Error('RUNTIME_PROBE_MIGRATION_INVALID');
    const file=await open('/opt/exhibitos/database/migrations/'+row.name,constants.O_RDONLY|constants.O_NOFOLLOW|constants.O_NONBLOCK);
    let raw;try{const st=await file.stat();if(!st.isFile()||st.nlink!==1||st.size>16*1024*1024)throw Error('RUNTIME_PROBE_MIGRATION_INVALID');raw=await file.readFile();}finally{await file.close();}
    if(createHash('sha256').update(raw).digest('hex')!==row.sha256)throw Error('RUNTIME_PROBE_MIGRATION_INVALID');
    const out=await open(directory+'/'+row.name,'wx',0o400);try{await out.writeFile(raw);await out.sync();}finally{await out.close();}
  }
  const {default:pg}=await import('/opt/exhibitos/node_modules/pg/lib/index.js');
  const {verifyRestoredInventory,verifyMigratedInventory}=await import('/opt/exhibitos/packages/storage/dist/service-backup.js');
  const {collectServiceInventory}=await import('/opt/exhibitos/packages/storage/dist/service-inventory.js');
  const {FileBlobStore}=await import('/opt/exhibitos/packages/storage/dist/blobs.js');
  const pool=new pg.Pool({connectionString:environment.DATABASE_URL,max:2,connectionTimeoutMillis:5000,statement_timeout:30000});
  const store=new FileBlobStore('/probe/blobs');
  const options={pool,manifestBytes:bytes,expectedManifestSha256:MIGRATION_INPUT.manifestSha256,snapshotSystemIdentifier:ready.physical.systemIdentifier};
  try{const original=await verifyRestoredInventory({...options,snapshot:c=>collectServiceInventory(c,store,{migrationDirectory:directory})});if(original.schemaSha256!==MIGRATION_INPUT.sourceSchemaSha256)throw Error('RUNTIME_PROBE_MIGRATION_INVALID');}finally{await pool.end();}
  migrationState={options,environment,store,collectServiceInventory,verifyMigratedInventory,pg};
}
async function migratedLogical() {
  const s=migrationState;if(!s)throw Error('RUNTIME_PROBE_MIGRATION_INVALID');
  const pool=new s.pg.Pool({connectionString:s.environment.DATABASE_URL,max:2,connectionTimeoutMillis:5000,statement_timeout:30000});
  try{return await s.verifyMigratedInventory({...s.options,pool,targetSchemaSha256:MIGRATION_INPUT.targetSchemaSha256,targetMigrations:MIGRATION_INPUT.catalog.migrations,snapshot:c=>s.collectServiceInventory(c,s.store,{migrationDirectory:'/opt/exhibitos/database/migrations'})});}finally{await pool.end();}
}
