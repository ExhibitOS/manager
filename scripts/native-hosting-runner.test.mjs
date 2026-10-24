// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {verifyPublicStatic,runNativeHosting,nativeHostingFailure} from './native-hosting-runner.mjs';
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
