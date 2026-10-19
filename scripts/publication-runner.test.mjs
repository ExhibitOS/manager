// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,copyFile,symlink,rm,readFile,lstat} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createServer} from 'node:http';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID,createHash} from 'node:crypto';
const manager=fileURLToPath(new URL('../',import.meta.url)),node=join(manager,'src-tauri/remote-runtime/generated/node'),runtime=join(manager,'src-tauri/publication-runtime/generated');
const token='n'.repeat(43),csrf='s'.repeat(43),password='synthetic-runner-password',tenant='70000000-0000-4000-8000-000000000001',id='70000000-0000-4000-8000-000000000002',other='70000000-0000-4000-8000-000000000003';
const item={publicationId:id,exhibitionId:other,title:'Runner synthetic exhibition',revisionSha256:'f'.repeat(64),publishedAt:'2026-10-17T00:00:00.000Z',status:'published'};
const sha=b=>createHash('sha256').update(b).digest('hex');
async function stage(t){
 try{await lstat(node);await lstat(join(runtime,'manifest.json'));}catch{t.skip('Prepared bundled Node and publication runtime required');return;}
 const temp=await mkdtemp(join(tmpdir(),'exhibitos-publication-runner-'));t.after(()=>rm(temp,{recursive:true,force:true}));
 await mkdir(join(temp,'scripts'));await mkdir(join(temp,'publication-runtime'));
 for(const file of ['publication-runner.mjs','publication-client.mjs'])await copyFile(join(manager,'scripts',file),join(temp,'scripts',file));
 // Test harness aliases the already verified real resource tree, without changing it.
 await symlink(runtime,join(temp,'publication-runtime/generated'));
 const run=async input=>{const child=spawn(node,['--disable-proto=delete','--no-warnings',join(temp,'scripts/publication-runner.mjs')],{cwd:temp,env:{},stdio:['pipe','pipe','pipe']});let stdout='',stderr='';child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);const timer=setTimeout(()=>child.kill('SIGKILL'),45000);const done=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',(code,signal)=>resolve({code,signal,stdout,stderr}));});child.stdin.on('error',()=>{});child.stdin.end(typeof input==='string'?input:JSON.stringify(input));try{const result=await done;assert.equal(result.signal,null);for(const secret of [password,token,csrf]){assert(!result.stdout.includes(secret));assert(!result.stderr.includes(secret));}assert.equal(result.stderr,'');const frame=JSON.parse(result.stdout);return {...result,frame};}finally{clearTimeout(timer);}};
 return {temp,run};
}
async function server(t,handle){
 let failure;const http=createServer(async(req,res)=>{try{await handle(req,res);}catch(e){failure=e;res.writeHead(500,{'content-type':'application/json'});res.end('{"error":"synthetic"}');}});
 await new Promise((resolve,reject)=>{http.once('error',reject);http.listen(0,'127.0.0.1',resolve);});t.after(async()=>{await new Promise(resolve=>http.close(resolve));if(failure)throw failure;});return 'http://127.0.0.1:'+http.address().port;
}
const json=(res,value,headers={})=>{res.writeHead(200,{'content-type':'application/json',...headers});res.end(JSON.stringify(value));};
test('bundled Node throw mode exposes locked glTF validator import incompatibility before network or export',async t=>{
 const s=await stage(t);if(!s)return;
 const entry=pathToFileURL(join(s.temp,'publication-runtime/generated/exporter.mjs')).href;
 const script='await import(process.argv[1]);process.stdout.write("IMPORTED")';
 const probe=spawnSync(node,['--disable-proto=throw','--no-warnings','--input-type=module','-e',script,entry],{cwd:s.temp,env:{},encoding:'utf8',timeout:15000});
 assert.equal(probe.error,undefined);assert.equal(probe.status,1);assert.equal(probe.stdout,'');
 assert.match(probe.stderr,/gltf-validator\/gltf_validator\.dart\.js:8/);
 assert.match(probe.stderr,/ERR_PROTO_ACCESS/);
 assert.match(probe.stderr,/Accessing Object\.prototype\.__proto__ has been disallowed/);
});
test('bundled Node staged runner reads catalog over synthetic loopback HTTP and logs out without secret output',async t=>{
 const s=await stage(t);if(!s)return;const calls=[];
 const origin=await server(t,async(req,res)=>{calls.push(req.url);assert.equal(req.headers.origin,origin);if(req.url==='/api/v1/auth/login'){let body='';for await(const chunk of req)body+=chunk;assert.deepEqual(JSON.parse(body),{tenantId:tenant,subject:'artist',password});assert.equal(req.headers.cookie,undefined);return json(res,{authenticated:true},{'set-cookie':'exhibitos_local_session='+token+'; Path=/; HttpOnly; SameSite=Strict'});}assert.equal(req.headers.cookie,'exhibitos_local_session='+token);if(req.url==='/api/v1/auth/session')return json(res,{id,userId:other,tenantId:tenant,role:'artist',csrfToken:csrf,expiresAt:'2099-01-01T00:00:00.000Z'});if(req.url==='/api/v1/auth/logout'){assert.equal(req.method,'POST');assert.equal(req.headers['x-csrf-token'],csrf);return json(res,{loggedOut:true});}assert.equal(req.url,`/api/v1/tenants/${tenant}/studio/publications`);json(res,{items:[item]});});
 const result=await s.run({action:'catalog',origin,tenantId:tenant,subject:'artist',password});assert.equal(result.code,0);assert.deepEqual(result.frame,{items:[item]});assert.deepEqual(calls,['/api/v1/auth/login','/api/v1/auth/session',`/api/v1/tenants/${tenant}/studio/publications`,'/api/v1/auth/logout']);
});
test('bundled runner emits one bounded fixed error frame for malformed or forbidden input',async t=>{const s=await stage(t);if(!s)return;for(const input of ['{secret-invalid-json',JSON.stringify({action:'other',secret:password}),'x'.repeat(16385)]){const result=await s.run(input);assert.equal(result.code,1);assert.deepEqual(Object.keys(result.frame),['errorCode']);assert.match(result.frame.errorCode,/^LOCAL_CMS_(?:REQUEST_INVALID|EXPORT_FAILED)$/);assert(result.stdout.length<128);}});
test('bundled runner exports synthetic GLB PNG WAV over anonymous HTTP with actual packaged Viewer and pinned manifest',async t=>{
 const s=await stage(t);if(!s)return;
 const{readOex,revisionHash,fixtureURL}=await import(pathToFileURL(join(runtime,'platform/node_modules/@exhibitos/spec/index.mjs')));
 const decoded=await readOex(await readFile(fixtureURL('oex/v1/examples/synthetic-media.oex'))),publicationId=randomUUID();
 const raw={publication:{id:publicationId,status:'published',revisionSha256:revisionHash(decoded.exhibition),publishedAt:new Date().toISOString()},exhibition:decoded.exhibition,assets:decoded.manifest.assets.map(a=>({assetId:a.id,mime:a.mime,url:`/api/v1/publications/${publicationId}/assets/${a.id}`}))};let projections=0;
 const origin=await server(t,async(req,res)=>{assert.equal(req.method,'GET');assert.equal(req.headers.cookie,undefined);assert.equal(req.headers.authorization,undefined);if(req.url===`/api/v1/publications/${publicationId}`){projections++;return json(res,raw);}const asset=decoded.manifest.assets.find(a=>req.url===`/api/v1/publications/${publicationId}/assets/${a.id}`);assert(asset);res.writeHead(200,{'content-type':asset.mime,'x-exhibitos-publication-revision':raw.publication.revisionSha256});res.end(decoded.files.get(asset.path));});
 const output=join(s.temp,'exported'),input={action:'export',origin,output,publicationId,expectedRevisionSha256:raw.publication.revisionSha256},result=await s.run(input);assert.equal(result.code,0);assert.equal(projections,2);assert.deepEqual(Object.keys(result.frame).sort(),['manifestSha256','publicationId','revisionSha256']);assert.equal(result.frame.publicationId,publicationId);assert.equal(result.frame.revisionSha256,raw.publication.revisionSha256);
 const bytes=await readFile(join(output,'static-export-manifest.json')),manifest=JSON.parse(bytes);assert.equal(sha(bytes),result.frame.manifestSha256);assert.equal(manifest.format,'exhibitos-static-publication-v1');for(const pin of manifest.files){const bytes=await readFile(join(output,pin.path));assert.equal(bytes.length,pin.bytes);assert.equal(sha(bytes),pin.sha256);}
 assert(manifest.files.some(f=>f.path==='THIRD_PARTY_NOTICES.txt'));assert(manifest.files.some(f=>f.path.startsWith('assets/navigation-camera-')));
 const again=await s.run(input);assert.equal(again.code,1);assert.deepEqual(again.frame,{errorCode:'LOCAL_CMS_EXPORT_FAILED'});assert.equal(sha(await readFile(join(output,'static-export-manifest.json'))),result.frame.manifestSha256);
});

test('publication delete mode removes prototype accessor and assignment cannot mutate object prototype',async()=>{
 const {spawnSync}=await import('node:child_process');
 const result=spawnSync(node,['--disable-proto=delete','--no-warnings','-e',`const assert=require('node:assert/strict');assert.equal(Object.getOwnPropertyDescriptor(Object.prototype,'__proto__'),undefined);const object={},before=Object.getPrototypeOf(object);assert.equal(object.__proto__,undefined);object.__proto__={injected:true};assert.equal(Object.getPrototypeOf(object),before);assert.equal(object.injected,undefined);process.stdout.write('PROTOTYPE_ACCESSOR_REMOVED');`],{encoding:'utf8',env:{}});
 assert.equal(result.status,0);assert.equal(result.stdout,'PROTOTYPE_ACCESSOR_REMOVED');assert.equal(result.stderr,'');
});
