// SPDX-License-Identifier: Apache-2.0
// Integrity/authority check only. Never grants rights, renews offline grants,
// creates a signing key, starts a writer or activates from a journal.
import {readFile} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash,createPrivateKey,createPublicKey} from 'node:crypto';
import {inspectionRequest,privateManifest} from './native-legacy-inspect.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
import {verifyFreezeSignature} from '../vendor/platform/freeze-signature.mjs';

const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const fail=()=>{throw Error('NATIVE_LEGACY_ACTIVATION_CHECK_FAILED');};
async function run(request){
 const runtime=new URL('../../native-runtime/',import.meta.url);
 if(hash(await readFile(new URL('manifest.json',runtime)))!==request.payloadSha256)fail();
 const manifestBytes=await privateManifest(request.manifestPath);
 if(hash(manifestBytes)!==request.manifestSha256)fail();
 const manifest=boundedJSON(manifestBytes.toString('utf8'),16*1024*1024);
 if(manifest.id!==request.backupId)fail();
 const restored=dirname(request.manifestPath),job=dirname(restored);
 const configuration=await privateManifest(resolve(restored,'configuration/manager-bundle-manifest.json'));
 const configurationEntry=manifest.files?.find(f=>f.role==='configuration'&&f.name==='manager-bundle-manifest.json');
 if(configurationEntry?.sha256!==hash(configuration)||configurationEntry.bytes!==configuration.length)fail();
 const declared=boundedJSON(configuration.toString('utf8'),1024*1024),url=new URL(declared.openUrl);
 // Match the currently admitted legacy route. HTTPS/other source kinds require
 // their actual route adapter; never silently rewrite their authority.
 if(url.origin!==`http://127.0.0.1:${declared.ports?.[0]}`||url.username||url.password)fail();
 const keyBytes=await privateManifest(resolve(job,'activation/freeze-signing-key.json'));
 let signingKey;
 try{
  const entry=manifest.files?.find(f=>f.role==='configuration'&&f.name==='freeze-signing-key.json');
  if(keyBytes.length>8192||entry?.sha256!==hash(keyBytes)||entry.bytes!==keyBytes.length)fail();
  const record=boundedJSON(keyBytes.toString('utf8'),8192);
  if(!record||Object.keys(record).sort().join(',')!=='name,privateKey,schemaVersion'||record.name!=='exhibitos-freeze-ed25519'||record.schemaVersion!=='1.0.0-draft.1'||typeof record.privateKey!=='string'||Buffer.from(record.privateKey,'base64').toString('base64')!==record.privateKey)fail();
  signingKey=createPrivateKey({key:Buffer.from(record.privateKey,'base64'),format:'der',type:'pkcs8'});
  if(signingKey.type!=='private'||signingKey.asymmetricKeyType!=='ed25519')fail();
 }finally{keyBytes.fill(0);}
 const publicKey=createPublicKey(signingKey).export({type:'spki',format:'der'});
 const authority={origin:url.origin,keyId:hash(publicKey),publicKey:publicKey.toString('base64')};
 const database=new URL(process.env.DATABASE_URL??'');
 if(!['postgres:','postgresql:'].includes(database.protocol)||database.hostname!=='127.0.0.1'||database.pathname!=='/exhibitos'||database.search||database.hash||!database.port||!database.username||!database.password)fail();
 const {default:pg}=await import(new URL('platform/node_modules/pg/lib/index.js',runtime));
 const pool=new pg.Pool({connectionString:database.href,max:1,connectionTimeoutMillis:5000,statement_timeout:30000});
 let count=0;
 try{
  const client=await pool.connect();let transaction=false,broken=false;
  try{
   await client.query('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY');transaction=true;
   if((await client.query('SELECT pg_try_advisory_xact_lock(82002) AS locked')).rows[0]?.locked!==true)fail();
   const identity=(await client.query('SELECT system_identifier::text AS system,current_database() AS database FROM pg_control_system()')).rows[0];
   if(identity?.system!==request.systemIdentifier||identity.database!==manifest.sourceIdentity.database||identity.system===manifest.sourceIdentity.systemIdentifier)fail();
   await client.query('DECLARE legacy_freeze_signatures NO SCROLL CURSOR FOR SELECT manifest,manifest_sha256,signature FROM public.exhibition_freezes ORDER BY tenant_id,id');
   for(;;){
    const rows=(await client.query('FETCH 128 FROM legacy_freeze_signatures')).rows;if(!rows.length)break;
    for(const row of rows){if(++count>10000)fail();verifyFreezeSignature(row,authority,signingKey);}
   }
   await client.query('CLOSE legacy_freeze_signatures');
  }finally{
   if(transaction)await client.query('ROLLBACK').catch(()=>{broken=true;});
   client.release(broken);if(broken)fail();
  }
 }finally{await pool.end();}
 return {format:1,operation:'legacy-activation-integrity-checked',candidateId:request.candidateId,backupId:request.backupId,manifestSha256:request.manifestSha256,payloadSha256:request.payloadSha256,systemIdentifier:request.systemIdentifier,origin:authority.origin,keyId:authority.keyId,freezeRowsVerified:count,nonzeroFreezeRowsVerified:count>0,rightsOrGrantRenewed:false};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 try{
  const chunks=[];let bytes=0;for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>16384)fail();chunks.push(chunk);}
  const result=JSON.stringify(await run(inspectionRequest(boundedJSON(Buffer.concat(chunks).toString('utf8'),16384))));
  if(Buffer.byteLength(result)>16384)fail();process.stdout.write(result);
 }catch{process.stdout.write(JSON.stringify({errorCode:'NATIVE_LEGACY_ACTIVATION_CHECK_FAILED'}));process.exitCode=1;}
}
