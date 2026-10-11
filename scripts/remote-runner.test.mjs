// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';import assert from 'node:assert/strict';
import {executeRemote,project} from './remote-runner.mjs';
const id='12345678-1234-4234-8234-123456789abc',hash='a'.repeat(64);
const request=action=>({requestId:id,action,oed:{},connection:{},maintenance:null,consent:{preserveVolumes:true,existingImagesOnly:true,externalWritersQuiesced:false,appsClosed:false}});
test('fixed fresh plan is consumed by actual action and closed output',async()=>{
 const calls=[];const plan={};const adapter={planSsh:async(...args)=>{calls.push(args);return plan;},applySsh:async(p,o)=>{assert.equal(p,plan);assert.equal(o.action,'status');return {version:1,state:'stopped',ready:false,healthVerified:false};}};
 const r=await executeRemote(request('status'),adapter);assert.deepEqual(r,{version:1,action:'status',state:'stopped',ready:false,healthVerified:false});assert.equal(calls.length,1);assert.equal(typeof calls[0][2].run,'function');
 assert.throws(()=>project('status',{version:1,state:'stopped',ready:false,healthVerified:false,token:'secret'}),/SSH_RESPONSE_INVALID/);
 assert.throws(()=>project('logs',{version:1,entries:[{at:1,code:'RAW secret'}],rawLogsIncluded:false}),/SSH_RESPONSE_INVALID/);
});
test('maintenance typed inputs and explicit consent reach public API without extra authority',async()=>{
 const input={maintenanceImage:'sha256:'+hash,keyPath:'/external/key.bin'},r=request('backup');r.maintenance=input;
 await assert.rejects(executeRemote(r,{}),/SSH_CONSENT_REQUIRED/);r.consent.externalWritersQuiesced=true;r.consent.appsClosed=true;
 let seen;const result=await executeRemote(r,{planSsh:async()=>({}),maintenanceSsh:async(p,o)=>{seen=o;return {version:1,action:'backup',state:'completed',jobId:id,backupId:id,authenticatedManifestSha256:hash,writersPaused:true,preservedVolumes:true};}});
 assert.equal(seen.input,input);assert.equal(seen.appsClosed,true);assert.equal(result.backupId,id);
 await assert.rejects(executeRemote({...request('status'),arbitraryCommand:'delete'},{}),/SSH_REQUEST_INVALID/);
});
test('abort travels through plan transport and does not report remote completion',async()=>{
 const abort=new AbortController();let signal;const adapter={planSsh:async(o,c,options)=>{await options.run(o,c,'fixed');abort.abort();return{};},runSsh:async(o,c,s,options)=>{signal=options.signal;return{};},applySsh:async()=>({version:1,state:'stopped',ready:false,healthVerified:false})};
 await assert.rejects(executeRemote(request('status'),adapter,abort.signal),/SSH_CANCELLED_UNCERTAIN/);assert.equal(signal,abort.signal);
 assert.throws(()=>project('rollback',{version:1,action:'rollback',state:'completed'},{}),/SSH_RESPONSE_INVALID/);
});

test('installed runner enters main under a path containing spaces without resolving credentials',async()=>{
 const {mkdtemp,mkdir,copyFile,symlink,rm}=await import('node:fs/promises');const {tmpdir}=await import('node:os');const {join}=await import('node:path');const {fileURLToPath}=await import('node:url');const {spawnSync}=await import('node:child_process');
 const root=await mkdtemp(join(tmpdir(),'exhibitos runner space '));try{await mkdir(join(root,'scripts'));await symlink(fileURLToPath(new URL('../vendor',import.meta.url)),join(root,'vendor'),'dir');const runner=join(root,'scripts','remote-runner.mjs');await copyFile(fileURLToPath(new URL('./remote-runner.mjs',import.meta.url)),runner);const actual=spawnSync(process.execPath,[runner],{input:'{}',encoding:'utf8',timeout:5000});assert.equal(actual.status,1);assert.equal(actual.stdout,'');assert.equal(actual.stderr.trim(),'SSH_REQUEST_INVALID');}finally{await rm(root,{recursive:true,force:true});}
});
