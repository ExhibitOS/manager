// SPDX-License-Identifier: Apache-2.0
import test from 'node:test';
import assert from 'node:assert/strict';
import {generateKeyPairSync} from 'node:crypto';
import {prepareConnection,connectRestored} from '../scripts/render-restore-provider.mjs';
const signing=generateKeyPairSync('ed25519').privateKey.export({type:'pkcs8',format:'der'}).toString('base64');
const expected={origin:'https://synthetic.onrender.com',envVars:[{key:'CLIENT_ORIGIN',value:'https://synthetic.pages.dev'},{key:'TENANT_ID',value:'10000000-0000-4000-8000-000000000001'},{key:'ADMIN_SUBJECT',value:'synthetic-owner'},{key:'ADMIN_PASSWORD',value:'synthetic-original-password'},{key:'FREEZE_SIGNING_KEY_BASE64',value:signing}]};
const originalURL='postgresql://u:synthetic-source@dpg-source-a/db',targetURL='postgresql://u:synthetic-target@dpg-target-a/db';
const operation={id:'10000000-0000-4000-8000-000000000010',sourceJobId:'10000000-0000-4000-8000-000000000011',source:{workspaceId:'tea-one',serviceId:'srv-source',postgresId:'dpg-source'},target:{workspaceId:'tea-one',postgresId:'dpg-target',createRequested:false},state:'verified',targetIdentitySha256:'a'.repeat(64)};
function fixture(options={}){
 let environment=[...expected.envVars,{key:'DATABASE_URL',value:originalURL}],suspended='suspended',currentDeploy={id:'dep-original',commit:{id:'b'.repeat(40)},status:'live'};const deploys=new Map([[currentDeploy.id,currentDeploy]]),calls=[];let checkpoint=null;
 const api=async(path,body,method='GET')=>{
  calls.push({path,method,body});
  if(path==='/postgres/dpg-source')return {id:'dpg-source',owner:{id:'tea-one'}};
  if(path==='/postgres/dpg-source/connection-info')return {internalConnectionString:originalURL};
  if(path==='/postgres/dpg-target')return {id:'dpg-target',owner:{id:'tea-one'},plan:'free',region:'frankfurt',version:'18',status:'available',expiresAt:'2099-01-01T00:00:00Z',ipAllowList:[{cidrBlock:'192.0.2.1/32'}]};
  if(path==='/postgres/dpg-target/connection-info')return {internalConnectionString:targetURL,externalConnectionString:'postgresql://u:synthetic-target@dpg-target-a.frankfurt-postgres.render.com/db'};
  if(path==='/services/srv-source')return {id:'srv-source',ownerId:'tea-one',type:'web_service',repo:'https://github.com/ExhibitOS/platform',suspended,autoDeployTrigger:'off',serviceDetails:{runtime:'node',plan:'free',url:expected.origin}};
  if(path==='/services/srv-source/env-vars?limit=100')return environment.map(envVar=>({envVar}));
  if(path==='/services/srv-source/env-vars'){assert.equal(method,'PUT');environment=structuredClone(body);return environment.map(envVar=>({envVar}));}
  if(path==='/services/srv-source/resume'){assert.equal(method,'POST');suspended='not_suspended';return {};}
  if(path==='/services/srv-source/suspend'){assert.equal(method,'POST');suspended='suspended';return {};}
  if(path==='/services/srv-source/deploys?limit=20')return [{deploy:currentDeploy}];
  if(path==='/services/srv-source/deploys'){assert.equal(method,'POST');assert.equal(body.commitId,'b'.repeat(40));currentDeploy={id:'dep-connected',commit:{id:body.commitId},status:'live'};deploys.set(currentDeploy.id,currentDeploy);if(options.lostDeploy)throw Error('HOSTED_RESULT_UNCERTAIN');return currentDeploy;}
  if(path==='/services/srv-source/rollback'){assert.equal(method,'POST');assert.equal(body.deployId,'dep-original');currentDeploy={id:'dep-rollback',commit:{id:'b'.repeat(40)},status:'live'};deploys.set(currentDeploy.id,currentDeploy);if(options.lostRollback)throw Error('HOSTED_RESULT_UNCERTAIN');return currentDeploy;}
  if(path.startsWith('/services/srv-source/deploys/')){if(options.foreignDuringRollback&&path.endsWith('/dep-rollback'))environment.push({key:'FOREIGN_LATE_EDIT',value:'retain-me'});return deploys.get(path.split('/').at(-1));}
  throw Error('unexpected provider route');
 };
 const fetch=async(url,init={})=>{
  if(options.healthFailure)return new Response('{}',{status:503});
  if(url.endsWith('/readiness'))return new Response(JSON.stringify({ready:true,protocolVersion:'1',services:['platform','api','database','web','storage'].map(name=>({name,status:'ready'}))}));
  if(url.endsWith('/auth/login')){assert.equal(init.method,'POST');assert.equal(init.headers.Origin,'https://synthetic.pages.dev');assert.equal(JSON.parse(init.body).subject,'synthetic-owner');return new Response('{}',{headers:{'set-cookie':'session=synthetic-session; HttpOnly'}});}
  if(url.endsWith('/auth/session'))return new Response(JSON.stringify({tenantId:expected.envVars.find(e=>e.key==='TENANT_ID').value}));
  throw Error('unexpected HTTP route');
 };
 return {api,calls,dependencies:{fetch,delay:async()=>{}},save:async c=>{checkpoint=structuredClone(c);},checkpoint:()=>checkpoint,environment:()=>environment,setForeignEnv:()=>{environment.push({key:'FOREIGN_EDIT',value:'preserve-user-edit'});},suspended:()=>suspended};
}
const identity=async(url,hash)=>{assert.equal(hash,operation.targetIdentitySha256);assert.equal(new URL(url).searchParams.get('sslmode'),'verify-full');};
test('explicit connection preserves full env and deploys exact observed commit, then proves identity/readiness/account',async()=>{const f=fixture(),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);const result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connected');assert.equal(result.commitId,plan.originalCommitId);assert.equal(result.backupTimeRuntimeRecorded,false);assert.equal(f.environment().find(e=>e.key==='DATABASE_URL').value,targetURL);assert.equal(f.environment().find(e=>e.key==='FREEZE_SIGNING_KEY_BASE64').value,signing);assert.equal(f.calls.filter(c=>c.method==='POST'&&c.path.endsWith('/deploys')).length,1);});
test('health failure restores exact original env and retained deploy, leaving original suspended',async()=>{const f=fixture({healthFailure:true}),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);const result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connection-restored');assert.equal(f.suspended(),'suspended');assert.deepEqual(f.environment(),plan.originalEnvironment);assert.equal(f.calls.filter(c=>c.path.endsWith('/rollback')).length,1);});
test('lost deploy response does not repeat deployment and returns via original connection',async()=>{const f=fixture({lostDeploy:true}),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);const result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connection-restored');assert.equal(f.calls.filter(c=>c.method==='POST'&&c.path.endsWith('/deploys')).length,1);assert.deepEqual(f.environment(),plan.originalEnvironment);});
test('lost rollback response leaves uncertain state and no blind second rollback',async()=>{const f=fixture({healthFailure:true,lostRollback:true}),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);let result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connection-recovery-required');result=await connectRestored(f.api,operation,plan,f.checkpoint(),f.save,identity,f.dependencies);assert.equal(result.state,'connection-recovery-required');assert.equal(f.calls.filter(c=>c.path.endsWith('/rollback')).length,1);});
test('foreign edit before switching is preserved with zero provider mutations',async()=>{const f=fixture(),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);f.setForeignEnv();const result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connection-recovery-required');assert.equal(f.calls.filter(c=>c.method!=='GET').length,0);assert.equal(f.environment().at(-1).key,'FOREIGN_EDIT');});
test('lost successful outcome resumes same live deployment by observation, without mutation',async()=>{const f=fixture(),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);const mutationCount=f.calls.filter(c=>c.method!=='GET').length;const result=await connectRestored(f.api,operation,plan,f.checkpoint(),f.save,identity,f.dependencies);assert.equal(result.state,'connected');assert.equal(f.calls.filter(c=>c.method!=='GET').length,mutationCount);});

test('immutable connection plan owns configuration bytes independently from zeroized input',async()=>{const f=fixture(),input=structuredClone(expected),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',input);input.envVars.forEach(e=>e.value='');assert.equal(plan.expectedConfiguration.envVars.find(e=>e.key==='ADMIN_PASSWORD').value,'synthetic-original-password');});

test('foreign env edit while rollback becomes live refuses terminal restoration without overwriting edit',async()=>{const f=fixture({healthFailure:true,foreignDuringRollback:true}),plan=await prepareConnection(f.api,operation,'192.0.2.1/32',expected);const result=await connectRestored(f.api,operation,plan,null,f.save,identity,f.dependencies);assert.equal(result.state,'connection-recovery-required');assert.equal(f.environment().find(e=>e.key==='FOREIGN_LATE_EDIT').value,'retain-me');assert.equal(f.checkpoint().phase,'connection-recovery-required');assert.equal(f.calls.filter(c=>c.path.endsWith('/env-vars')&&c.method==='PUT').length,2);});
