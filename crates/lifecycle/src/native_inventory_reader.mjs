// SPDX-License-Identifier: Apache-2.0
// Fixed native reader: no caller-provided health success or physical DB identity.
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {realpath} from 'node:fs/promises';
import {FileBlobStore,collectServiceInventory,verifyRestoredInventory} from '/opt/exhibitos/packages/storage/dist/index.js';
const require=createRequire('/opt/exhibitos/package.json'),{Pool}=require('pg');
const [sha,expectedSystem]=process.argv.slice(1);let size=0,chunks=[];
for await(const c of process.stdin){size+=c.length;if(size>16*1024*1024)throw Error('UPDATE_INPUT_INVALID');chunks.push(c);}
const raw=Buffer.concat(chunks);
if(!/^[a-f0-9]{64}$/.test(sha??'')||createHash('sha256').update(raw).digest('hex')!==sha)throw Error('UPDATE_INPUT_INVALID');
if(process.env.BLOB_ROOT!=='/data/blobs'||await realpath('/data/blobs')!=='/data/blobs')throw Error('UPDATE_BLOB_ROOT_INVALID');
const pool=new Pool({connectionString:process.env.DATABASE_URL,connectionTimeoutMillis:15000,statement_timeout:60000});
try{
 const r=await pool.query('SELECT system_identifier::text AS identifier FROM pg_control_system()'),system=r.rows[0]?.identifier;
 if(!/^[1-9][0-9]{0,19}$/.test(system??''))throw Error('UPDATE_DATABASE_ID_INVALID');
 if(expectedSystem!==undefined&&system!==expectedSystem)throw Error('UPDATE_DATABASE_ID_MISMATCH');
 const blobs=new FileBlobStore('/data/blobs');
 const proof=await verifyRestoredInventory({pool,manifestBytes:raw,expectedManifestSha256:sha,snapshotSystemIdentifier:system,snapshot:c=>collectServiceInventory(c,blobs,{migrationDirectory:'/opt/exhibitos/database/migrations/'})});
 if(proof.currentInventoryVerified!==true||proof.preflightVerified!==false||proof.updateExecuted!==false)throw Error('UPDATE_INVENTORY_UNVERIFIED');
 console.log(JSON.stringify(proof));
}catch(e){const code=/^[A-Z][A-Z0-9_]{0,79}$/.test(e?.message??'')?e.message:'UPDATE_ROLLBACK_HEALTH_FAILED';process.stderr.write(code+'\n');process.exitCode=1;}
finally{await pool.end();}
