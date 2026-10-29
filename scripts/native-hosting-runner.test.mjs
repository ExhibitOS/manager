// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {verifyPublicStatic,runNativeHosting,nativeHostingFailure} from './native-hosting-runner.mjs';
import {pagesStaticCachePolicy,pagesDeploymentIdentity,STATIC_DATA_CACHE_PROFILE} from '../vendor/deployment/src/pages-render-worker.mjs';
import {executeHosted,inspectStaticBundle} from '../vendor/deployment/src/pages-render.mjs';
const data=Buffer.from('synthetic approved public asset');
const bundle={manifestSha256:'a'.repeat(64),files:[{path:'assets/synthetic.glb',bytes:data.length,sha256:createHash('sha256').update(data).digest('hex')}]};
const ready={provider:'cloudflare',state:'ready',projectName:'exhibition-fixture',url:'https://exhibition-fixture.pages.dev'};
test('public completion requires exact fetched bytes with redirects and credentials disabled',async()=>{
 const calls=[];const verified=await verifyPublicStatic(bundle,ready,async(url,options)=>{calls.push({url,options});return new Response(data);});
 assert.deepEqual(verified,{manifestSha256:bundle.manifestSha256,files:1,bytes:data.length});
 assert.equal(calls[0].url,ready.url+'/assets/synthetic.glb');
 assert.equal(calls[0].options.redirect,'manual');assert.equal(calls[0].options.credentials,'omit');assert.equal(calls[0].options.referrerPolicy,'no-referrer');
 assert.deepEqual(Object.keys(calls[0].options.headers),['Cache-Control']);
 const empty={...bundle,files:[{path:'assets/empty.txt',bytes:0,sha256:createHash('sha256').update('').digest('hex')}]};
 assert.equal((await verifyPublicStatic(empty,ready,async()=>new Response(''))).bytes,0);
 for(const response of [new Response(data,{status:302,headers:{Location:'https://other.invalid'}}),new Response(data,{status:404}),new Response(data.subarray(1)),new Response(Buffer.alloc(data.length)),new Response(Buffer.alloc(data.length+1))]){
  await assert.rejects(verifyPublicStatic(bundle,ready,async()=>response),/HOSTED_PUBLIC_CONTENT_(?:MISMATCH|UNAVAILABLE)/);
 }
});
test('public verification refuses unrelated origins and unsafe names before fetching',async()=>{
 let fetched=false;const fetch=async()=>{fetched=true;throw Error('must not fetch');};
 for(const url of ['https://other.pages.dev','http://exhibition-fixture.pages.dev','https://exhibition-fixture.pages.dev@other.invalid','https://exhibition-fixture.pages.dev/path'])await assert.rejects(verifyPublicStatic(bundle,{...ready,url},fetch),/HOSTED_RESPONSE_INVALID/);
 for(const path of ['../secret','assets/../secret','/absolute','https://other.invalid','assets/%2e%2e/secret'])await assert.rejects(verifyPublicStatic({...bundle,files:[{...bundle.files[0],path}]},ready,fetch),/HOSTED_STATIC_BUNDLE_INVALID/);
 assert.equal(fetched,false);
});
test('native entry refuses Render, dynamic deployment and unreviewed request fields before executing',async()=>{
 let executed=false;const options={execute:async()=>{executed=true;return {...ready,accounts:[]};}};
 const request={provider:'cloudflare',action:'accounts',staticOnly:true},tokens={cloudflare:'synthetic-token-not-a-secret'};
 for(const change of [{provider:'render'},{staticOnly:false},{action:'deploy-api'},{runtime:{}},{sourcePath:'/unreviewed'}])await assert.rejects(runNativeHosting({...request,...change},tokens,options),/HOSTED_REQUEST_INVALID/);
 assert.equal(executed,false);assert.equal((await runNativeHosting(request,tokens,options)).provider,'cloudflare');
});
test('native error frame never returns unknown exception text',()=>{
 assert.equal(nativeHostingFailure(Error('HOSTED_PUBLIC_CONTENT_MISMATCH')),'HOSTED_PUBLIC_CONTENT_MISMATCH');
 for(const error of [Error('token=synthetic-secret'),Error('/private/path'),Error('HOSTED_PUBLIC_CONTENT_MISMATCH\nSQL'),{code:'ENOENT'},null])assert.equal(nativeHostingFailure(error),'HOSTED_OPERATION_UNCERTAIN');
});
test('cache policy selects only exact content addressed public data, deterministically bounded',()=>{
 const descriptors=Array.from({length:102},(_,i)=>{const sha256=i.toString(16).padStart(64,'0');return {path:`data/${sha256}.bin`,sha256};});
 const policy=pagesStaticCachePolicy([...descriptors.reverse(),descriptors[0],{path:'index.html',sha256:'a'.repeat(64)},{path:`data/${'b'.repeat(64)}.json`,sha256:'c'.repeat(64)},{path:'data/*',sha256:'d'.repeat(64)}]);
 assert.equal(policy.paths.length,100);assert.deepEqual(policy.paths,[...policy.paths].sort());
 assert.equal(policy.headers,policy.paths.map(path=>`/${path}\n  Cache-Control: public, max-age=86400, immutable\n`).join('\n'));
 assert.deepEqual(pagesStaticCachePolicy([]),{paths:[],headers:''});
});
test('cache profile binds deployment identity while legacy identities remain unchanged',()=>{
 const manifest='a'.repeat(64), policyHash=createHash('sha256').update('').digest('hex');
 assert.equal(pagesDeploymentIdentity(manifest,{staticOnly:true}).hash,manifest.slice(0,40));
 const request={staticOnly:true,staticCacheProfile:STATIC_DATA_CACHE_PROFILE,staticCachePolicySha256:policyHash};
 assert.equal(pagesDeploymentIdentity(manifest,request).hash,createHash('sha256').update(`${manifest}\n${STATIC_DATA_CACHE_PROFILE}\n${policyHash}`).digest('hex').slice(0,40));
 assert.notEqual(pagesDeploymentIdentity(manifest,request).hash,pagesDeploymentIdentity(manifest,{...request,staticCachePolicySha256:'b'.repeat(64)}).hash);
 for(const bad of [{staticCacheProfile:'unknown',staticCachePolicySha256:policyHash},{staticCacheProfile:STATIC_DATA_CACHE_PROFILE},{staticCachePolicySha256:policyHash}])assert.throws(()=>pagesDeploymentIdentity(manifest,bad),/HOSTED_REQUEST_INVALID/);
});
test('new cache profile requires actual public response headers as well as exact bytes',async()=>{
 const digest=createHash('sha256').update(data).digest('hex'),path=`data/${digest}.bin`;
 const checked={...bundle,files:[{path,bytes:data.length,sha256:digest}],cachePolicy:{paths:[path]}};
 await assert.rejects(verifyPublicStatic(checked,ready,async()=>new Response(data),STATIC_DATA_CACHE_PROFILE),/HOSTED_PUBLIC_CONTENT_MISMATCH/);
 for(const value of ['public, max-age=86400, immutable, no-store','public, max-age=86400, immutable, private','public, max-age=86400, immutable, max-age=0','public, public, max-age=86400, immutable'])await assert.rejects(verifyPublicStatic(checked,ready,async()=>new Response(data,{headers:{'Cache-Control':value}}),STATIC_DATA_CACHE_PROFILE),/HOSTED_PUBLIC_CONTENT_MISMATCH/);
 await verifyPublicStatic(checked,ready,async()=>new Response(data,{headers:{'Cache-Control':'public, max-age=86400, immutable'}}),STATIC_DATA_CACHE_PROFILE);
});
test('profiled reconciliation uses only GET and never selects a legacy deployment',async t=>{
 const root=await mkdtemp(join(tmpdir(),'manager-cache-reconcile-'));t.after(()=>rm(root,{recursive:true,force:true}));
 const sha=v=>createHash('sha256').update(v).digest('hex'),id='00000000-0000-4000-8000-000000000001',revision='b'.repeat(64);
 const publication=JSON.stringify({publication:{id,status:'published',revisionSha256:revision}}),descriptor={path:'publication.json',bytes:Buffer.byteLength(publication),sha256:sha(publication)};
 const index=JSON.stringify({version:1,publication:descriptor,assets:[]}),indexSha=sha(index),html=`<html><head><meta name="exhibitos-static-index" content="${indexSha}"></head></html>`;
 const files={'publication.json':publication,'static-publication-index.json':index,'index.html':html};
 for(const [path,bytes] of Object.entries(files))await writeFile(join(root,path),bytes);
 const manifest={format:'exhibitos-static-publication-v1',publicationId:id,revisionSha256:revision,indexSha256:indexSha,files:Object.entries(files).map(([path,bytes])=>({path,bytes:Buffer.byteLength(bytes),sha256:sha(bytes)}))};
 await writeFile(join(root,'static-export-manifest.json'),JSON.stringify(manifest));const bundle=await inspectStaticBundle(root);
 const request={provider:'cloudflare',action:'reconcile',accountId:'a'.repeat(32),projectName:'exhibition-fixture',sourcePath:root,expectedManifestSha256:bundle.manifestSha256,staticOnly:true,staticCacheProfile:STATIC_DATA_CACHE_PROFILE,staticCachePolicySha256:sha(bundle.cachePolicy.headers)};
 const legacy=bundle.manifestSha256.slice(0,40),profiled=pagesDeploymentIdentity(bundle.manifestSha256,request).hash;assert.notEqual(legacy,profiled);
 let calls=0;const response=result=>new Response(JSON.stringify({success:true,result}),{headers:{'content-type':'application/json'}});
 const fetch=async(url,init)=>{calls++;assert.equal(init.method,'GET');if(url.endsWith('/exhibition-fixture'))return response({name:'exhibition-fixture'});return response([{id:'legacy',deployment_trigger:{metadata:{commit_hash:legacy}},latest_stage:{status:'success'}}]);};
 const result=await executeHosted(request,{tokens:{cloudflare:'synthetic-token'},fetch});assert.equal(result.state,'uncertain');assert.equal(result.deploymentId,'pending-'+profiled);assert.ok(calls>0);
});
test('marked work stays gated; only explicit deploy repairs observed header mismatch',async t=>{
 const root=await mkdtemp(join(tmpdir(),'manager-cache-marker-'));t.after(()=>rm(root,{recursive:true,force:true}));await mkdir(join(root,'data'));
 const sha=v=>createHash('sha256').update(v).digest('hex'),id='00000000-0000-4000-8000-000000000001',revision='b'.repeat(64);
 const pub=JSON.stringify({publication:{id,status:'published',revisionSha256:revision}}),pubHash=sha(pub),path=`data/${pubHash}.json`,index=JSON.stringify({version:1,publication:{path,bytes:Buffer.byteLength(pub),sha256:pubHash},assets:[]}),indexHash=sha(index);
 const headers=`/${path}\n  Cache-Control: public, max-age=86400, immutable\n`,marker=JSON.stringify({version:1,profile:'exhibitos-static-cache-v1',indexSha256:indexHash,headersSha256:sha(headers)});
 const files={'index.html':`<html><head><meta name="exhibitos-static-index" content="${indexHash}"></head></html>`,'static-publication-index.json':index,[path]:pub,'static-cache-policy.json':marker};
 for(const [name,bytes] of Object.entries(files))await writeFile(join(root,name),bytes);
 const manifest={format:'exhibitos-static-publication-v1',publicationId:id,revisionSha256:revision,indexSha256:indexHash,files:Object.entries(files).map(([path,bytes])=>({path,bytes:Buffer.byteLength(bytes),sha256:sha(bytes)}))};await writeFile(join(root,'static-export-manifest.json'),JSON.stringify(manifest));
 const request={provider:'cloudflare',action:'deploy-static',accountId:'a'.repeat(32),projectName:ready.projectName,sourcePath:root,expectedManifestSha256:sha(JSON.stringify(manifest)),staticOnly:true},tokens={cloudflare:'synthetic-token'};
 let calls=0,repaired=false,corrupt=false;const execute=async(req,opts)=>{calls++;assert.equal(req.staticCacheMarkerEnabled,true);if(opts.markerRepair){assert.deepEqual(opts.markerRepair,{deploymentId:'old'});repaired=true;return {...ready,deploymentId:'new'};}return {...ready,deploymentId:'old',reusedCurrent:true};};
 const fetch=async(url)=>{const name=new URL(url).pathname.slice(1),body=files[name];assert.notEqual(body,undefined);return new Response(corrupt&&name===path?'wrong':body,{headers:repaired&&name===path?{'cache-control':'public, max-age=86400, immutable'}:{}});};
 await assert.rejects(runNativeHosting(request,tokens,{execute,fetch}),/HOSTED_CACHE_POLICY_NOT_READY/);assert.equal(calls,0);
 await assert.rejects(runNativeHosting({...request,action:'reconcile'},tokens,{execute,fetch,markerDraft:true}),/HOSTED_CACHE_HEADERS_MISMATCH/);assert.equal(calls,1);
 calls=0;corrupt=true;await assert.rejects(runNativeHosting(request,tokens,{execute,fetch,markerDraft:true}),/HOSTED_PUBLIC_CONTENT_MISMATCH/);assert.equal(calls,1);
 calls=0;corrupt=false;const result=await runNativeHosting(request,tokens,{execute,fetch,markerDraft:true});assert.equal(calls,2);assert.equal(result.deploymentId,'new');assert.equal(result.verification.manifestSha256,request.expectedManifestSha256);
});
