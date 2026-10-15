// SPDX-License-Identifier: Apache-2.0
import {describe,expect,it} from 'vitest';
import {cloudConfiguration,cloudSummary,deploymentDescriptor,descriptorText,type DeploymentInput,type CloudInput} from './deployment-wizard';
const image='ghcr.io/exhibitos/platform@sha256:'+'a'.repeat(64),id='70000000-0000-4000-8000-000000000001',date='2026-10-11T20:00:00Z';
const database={source:'environment' as const,variable:'EXHIBITOS_DATABASE_PASSWORD'},ssh={source:'environment' as const,variable:'EXHIBITOS_SSH_KEY'};
const server={mode:'server' as const,image,database,ssh,host:'host.example.invalid',port:22,user:'exhibitos'};
const cloud:CloudInput={provider:'aws',account_id:'123456789012',region:'eu-central-1',project:'exhibition-test',ami_id:'ami-12345678',instance_type:'t3.small',subnet_id:'subnet-12345678',security_group_ids:['sg-12345678'],key_name:'exhibition-key',root_gib:20};
describe('Deployment preparation without execution authority',()=>{
 it('produces bounded local OED with the existing runtime storage contract',()=>{
  const oed=deploymentDescriptor({mode:'local',image,database},id,date);
  expect(oed.target).toEqual({type:'local-compose'});expect(oed.runtime).toEqual({image,httpPort:13200,replicas:1});
  expect(oed.storage.metadata).toMatchObject({host:'database',database:'exhibitos',passwordSecretRef:'database-password'});
  expect(oed.secretRefs).toEqual([{id:'database-password',...database}]);
  expect(JSON.parse(descriptorText(oed))).toEqual(oed);
 });
 it('produces server OED with only declared environment or keychain locators',()=>{
  const oed=deploymentDescriptor(server,id,date);expect(oed.target).toEqual({type:'generic-ssh',host:server.host,port:22,user:'exhibitos',sshKeySecretRef:'ssh-key'});
  expect(oed.secretRefs).toEqual([{id:'database-password',...database},{id:'ssh-key',...ssh}]);
  const keyed=deploymentDescriptor({...server,database:{source:'os-keychain',service:'exhibitos.deployment',account:'database'},ssh:{source:'os-keychain',service:'exhibitos.deployment',account:'ssh'}},id,date);
  expect(keyed.secretRefs).toHaveLength(2);expect(descriptorText(keyed)).not.toContain('passwordValue');
 });
 it('preserves packager content IDs through the local and vendored server contracts',async()=>{
  const contentId='sha256:'+'b'.repeat(64);
  const local=deploymentDescriptor({mode:'local',image:contentId,database},id,date);
  const remote=deploymentDescriptor({...server,image:contentId},id,date);
  expect(local.runtime.image).toBe(contentId);expect(remote.runtime.image).toBe(contentId);
  const {validateSshDescriptor}=await import(new URL('../vendor/deployment/src/generic-ssh.mjs',import.meta.url).href);
  expect(validateSshDescriptor(remote).runtime.image).toBe(contentId);
  for(const invalid of ['sha256:'+'0'.repeat(64),contentId+'\n',contentId+' ',contentId.toUpperCase(),'sha256:'+'b'.repeat(63)]){
   expect(()=>deploymentDescriptor({...server,image:invalid},id,date)).toThrow('DEPLOYMENT_INPUT_INVALID');
   expect(()=>validateSshDescriptor({...remote,runtime:{...remote.runtime,image:invalid}})).toThrow('SSH_RUNTIME_UNSUPPORTED');
  }
 });
 it('refuses unpinned/synthetic images, remote command fields, root user and invalid ports',()=>{
  for(const input of [{...server,image:'ghcr.io/exhibitos/platform:latest'},{...server,image:image.replace(/a{64}/,'0'.repeat(64))},{...server,user:'root'},{...server,host:'host; command'},{...server,port:0},{...server,port:65536},{...server,command:'echo synthetic'}])expect(()=>deploymentDescriptor(input,id,date)).toThrow('DEPLOYMENT_INPUT_INVALID');
  expect(()=>deploymentDescriptor(server,id,'2026-02-30T00:00:00Z')).toThrow();
 });
 it('refuses plaintext secret fields, duplicate locators and unsupported local keychain',()=>{
  for(const input of [{...server,database:{...database,value:'synthetic-not-a-real-secret'}},{...server,ssh:database},{mode:'local',image,database:{source:'os-keychain',service:'exhibitos',account:'db'}},{...server,ssh:{source:'environment',variable:'NAME=synthetic'}},{...server,database:{source:'os-keychain',account:'db'}},{...server,ssh:{source:'os-keychain',service:'exhibitos'}}])expect(()=>deploymentDescriptor(input as DeploymentInput,id,date)).toThrow();
 });
 it('produces only the exact AWS configuration and truthful unenforced dry-run summary',()=>{
  const config=cloudConfiguration(cloud);expect(config).toEqual(cloud);expect(config).not.toBe(cloud);
  expect(cloudSummary(cloud)).toMatchObject({pins:{opentofu:'1.12.0',aws:'6.42.0'},cost:{monthlyEstimate:null,enforced:false,initialAllowance:0},credentialsIncluded:false,runtimeInstalled:false,healthVerified:false,fallback:'generic-ssh'});
 });
 it('refuses credential values, unsupported provider and malformed resource configuration',()=>{
  for(const input of [{...cloud,token:'synthetic'},{...cloud,provider:'unsupported'},{...cloud,root_gib:19},{...cloud,security_group_ids:['arbitrary']},{...cloud,account_id:'bad'},{...cloud,project:undefined},{...cloud,key_name:undefined}])expect(()=>cloudConfiguration(input as CloudInput)).toThrow();
 });
});
