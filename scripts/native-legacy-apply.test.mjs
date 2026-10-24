// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {deriveFixedTarget,commitFixedTarget,schemaRequest} from './native-legacy-apply.mjs';

const digest='a'.repeat(64),lastHash='cef351fc481576cac99f8b9653e6343b505b38f079d0f0789bf00367d2d69fab';
const canonical=value=>JSON.stringify(value,(_k,v)=>v&&typeof v==='object'&&!Array.isArray(v)?Object.fromEntries(Object.entries(v).sort()):v);
function fixture({applied=true,nonempty=false,alterOriginal=false,alterTarget=false,unknownMigration=false,dropFailure=false,lock=false,rollbackFailure=false}={}){
 const catalog=Array.from({length:16},(_,i)=>({name:i===15?'016_local_workspaces.sql':`${String(i+1).padStart(3,'0')}_fixed.sql`,sha256:i===15?lastHash:digest}));
 const original={createdAt:'original',schemaVersion:'inventory-v1',schemaDigest:digest,migrations:catalog.slice(0,15),tables:[{name:'schema_migrations',rowCount:15,sha256:'b'.repeat(64)},{name:'original_rows',rowCount:2,sha256:'c'.repeat(64)}],objects:[],references:[],issues:[]};
 const target={...structuredClone(original),schemaDigest:'d'.repeat(64),migrations:catalog};
 const calls=[];let migrated=false;
 const client={async query(sql,args){calls.push({sql,args});
  if(sql.startsWith('SELECT pg_try'))return {rows:[{locked:!lock}]};
  if(sql.includes('pg_control_system'))return {rows:[{system:'2',database:'exhibitos'}]};
  if(sql.startsWith('SELECT name,sha256'))return {rows:unknownMigration?[...catalog,{name:'017_unknown.sql',sha256:digest}]:applied?catalog:catalog.slice(0,15)};
  if(sql.startsWith('SELECT NOT EXISTS'))return {rows:[{empty:!nonempty}]};
  if(sql.startsWith('DELETE'))return {rowCount:1,rows:[{name:'016_local_workspaces.sql'}]};
  if(dropFailure&&sql.startsWith('DROP'))throw Error('dependent object');
  if(rollbackFailure&&sql==='ROLLBACK')throw Error('connection lost');
  return {rows:[]};
 },release(broken){calls.push({release:broken});}};
 const context={manifest:{sourceIdentity:{systemIdentifier:'1',database:'exhibitos'},inventory:original},catalog,directory:'/fixed',store:{},systemIdentifier:'2',canonical,
  async collect(_client,_store,options){
   if(options.migrationCatalog){const value=structuredClone(original);value.createdAt='fresh';if(alterOriginal)value.tables[0].sha256='e'.repeat(64);return value;}
   const value=structuredClone(target);if(!migrated&&alterTarget)value.schemaDigest='f'.repeat(64);return value;
  },async migrate(){migrated=true;calls.push({migrate:true});}};
 return {client,context,calls};
}
test('closed schema request refuses journal authority, unknown operations and extra fields',()=>{
 const binding={operation:'reconcile',candidateId:'12345678-1234-1234-1234-123456789012',backupId:'22345678-1234-1234-1234-123456789012',manifestPath:'/fixed/manifest.json',manifestSha256:digest,payloadSha256:digest,systemIdentifier:'2'};
 assert.equal(schemaRequest(binding).operation,'reconcile');
 for(const extra of [{operation:'activate'},{targetSchemaSha256:digest},{receipt:{applied:true}}])assert.throws(()=>schemaRequest({...binding,...extra}));
});
test('known original state derives target without destructive SQL and always rolls back',async()=>{
 const f=fixture({applied:false});const result=await deriveFixedTarget(f.client,f.context);assert.equal(result.applied,false);
 assert(!f.calls.some(c=>c.sql?.startsWith('DROP')||c.sql==='COMMIT'));assert.equal(f.calls.at(-2).sql,'ROLLBACK');assert.deepEqual(f.calls.at(-1),{release:false});
});
test('committed or unknown-outcome state temporarily removes only exact016 then rolls back',async()=>{
 const f=fixture();assert.equal((await deriveFixedTarget(f.client,f.context)).applied,true);
 assert(f.calls.some(c=>c.sql==='DROP TABLE public.local_workspaces'));
 const removed=f.calls.find(c=>c.sql?.startsWith('DELETE'));assert.deepEqual(removed.args,['016_local_workspaces.sql',lastHash]);
 assert(!f.calls.some(c=>c.sql?.includes('CASCADE')||c.sql==='COMMIT'));assert.equal(f.calls.at(-2).sql,'ROLLBACK');
 assert(f.calls.findIndex(c=>c.sql?.includes('82002'))<f.calls.findIndex(c=>c.sql?.includes('82001')));
});
for(const [name,options] of Object.entries({nonempty:{nonempty:true},unknownMigration:{unknownMigration:true},originalHistoryChanged:{alterOriginal:true},targetSchemaChanged:{alterTarget:true},dependentObject:{dropFailure:true},busy:{lock:true}})){
 test(`reconciliation refuses ${name}, preserves transactional boundary`,async()=>{
  const f=fixture(options);await assert.rejects(deriveFixedTarget(f.client,f.context));assert(!f.calls.some(c=>c.sql==='COMMIT'));assert.equal(f.calls.at(-2).sql,'ROLLBACK');
  if(name==='nonempty'||name==='unknownMigration'||name==='busy')assert(!f.calls.some(c=>c.sql?.startsWith('DROP')));
 });
}
test('failed rollback discards the client and cannot yield a capability',async()=>{
 const f=fixture({rollbackFailure:true});await assert.rejects(deriveFixedTarget(f.client,f.context));assert.deepEqual(f.calls.at(-1),{release:true});
});
test('commit uses a fresh original inventory check and the freshly derived schema',async()=>{
 const first=fixture({applied:false}),derived=await deriveFixedTarget(first.client,first.context);
 const f=fixture({applied:false});await commitFixedTarget(f.client,f.context,derived.targetSchemaSha256);
 assert.equal(f.calls.filter(c=>c.sql==='COMMIT').length,1);assert(!f.calls.some(c=>c.sql==='ROLLBACK'));assert.deepEqual(f.calls.at(-1),{release:false});
});
test('commit original-state mismatch never submits COMMIT',async()=>{
 const f=fixture({applied:false,alterOriginal:true});await assert.rejects(commitFixedTarget(f.client,f.context,digest));
 assert(!f.calls.some(c=>c.sql==='COMMIT'));assert.equal(f.calls.at(-2).sql,'ROLLBACK');
});
test('commit acknowledgment loss remains an error even when cleanup succeeds',async()=>{
 const first=fixture({applied:false}),derived=await deriveFixedTarget(first.client,first.context);
 const f=fixture({applied:false}),query=f.client.query.bind(f.client);
 f.client.query=async(sql,args)=>{if(sql==='COMMIT'){f.calls.push({sql});throw Error('commit outcome unknown');}return query(sql,args);};
 await assert.rejects(commitFixedTarget(f.client,f.context,derived.targetSchemaSha256),/commit outcome unknown/);
 assert.equal(f.calls.filter(c=>c.sql==='COMMIT').length,1);assert.equal(f.calls.at(-2).sql,'ROLLBACK');assert.deepEqual(f.calls.at(-1),{release:false});
});
