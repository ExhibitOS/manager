// SPDX-License-Identifier: Apache-2.0
export type CredentialReference = {source:'environment'; variable:string} | {source:'os-keychain'; service:string; account:string};
export type DeploymentInput = {mode:'local'|'server'; image:string; database:CredentialReference; host?:string; port?:number; user?:string; ssh?:CredentialReference};
export type CloudInput = {provider:'aws'; account_id:string; region:string; project:string; ami_id:string; instance_type:'t3.small'|'t3.medium'; subnet_id:string; security_group_ids:string[]; key_name:string; root_gib:number};
const reject=()=>{throw new Error('DEPLOYMENT_INPUT_INVALID');};
const closed=(value:object,keys:string[])=>value!==null && typeof value==='object' && !Array.isArray(value) && Object.keys(value).every(k=>keys.includes(k));
function reference(id:string,input:CredentialReference){
 if(input?.source==='environment' && closed(input,['source','variable']) && typeof input.variable==='string' && /^EXHIBITOS_[A-Z0-9_]{1,64}$/.test(input.variable))return {id,source:input.source,variable:input.variable};
 if(input?.source==='os-keychain' && closed(input,['source','service','account']) && typeof input.service==='string' && typeof input.account==='string' && /^[a-z][a-z0-9.-]{0,127}$/.test(input.service) && /^[a-z][a-z0-9-]{0,63}$/.test(input.account))return {id,source:input.source,service:input.service,account:input.account};
 return reject();
}
/** Public OED restricted to the actual local/Generic SSH consumer. Never resolves a credential. */
export function deploymentDescriptor(input:DeploymentInput,id=crypto.randomUUID(),createdAt=new Date().toISOString()){
 if(!closed(input,['mode','image','database','host','port','user','ssh']) || !['local','server'].includes(input.mode) || typeof input.image!=='string' || input.image.length>512 || !/^(?:sha256:|[a-z0-9][a-z0-9./:_-]+@sha256:)[a-f0-9]{64}$(?![\s\S])/.test(input.image) || /(?:^|@)sha256:0{64}$/.test(input.image) || !/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(id) || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:[0-5]\d(?:\.\d{3})?Z$/.test(createdAt) || !Number.isFinite(Date.parse(createdAt)) || new Date(createdAt).toISOString().replace('.000Z','Z')!==createdAt.replace('.000Z','Z'))return reject();
 const database=reference('database-password',input.database);
 let target:{type:'local-compose'}|{type:'generic-ssh';host:string;port:number;user:string;sshKeySecretRef:string}={type:'local-compose'};
 const secretRefs=[database];
 if(input.mode==='local'){
  if(input.database.source!=='environment' || input.host!==undefined || input.port!==undefined || input.user!==undefined || input.ssh!==undefined)return reject();
 }else{
  if(typeof input.host!=='string' || input.host.length>253 || !/^[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?$/.test(input.host) || !Number.isInteger(input.port) || input.port!<1 || input.port!>65535 || typeof input.user!=='string' || !/^[a-z_][a-z0-9_-]{0,31}$/.test(input.user) || input.user==='root' || !input.ssh)return reject();
  const ssh=reference('ssh-key',input.ssh);
  if(JSON.stringify({...database,id:''})===JSON.stringify({...ssh,id:''}))return reject();
  secretRefs.push(ssh);target={type:'generic-ssh',host:input.host,port:input.port!,user:input.user,sshKeySecretRef:'ssh-key'};
 }
 return {schemaVersion:'1.0.0-draft.1',kind:'deployment',id,createdAt,target,runtime:{image:input.image,httpPort:13200,replicas:1},storage:{metadata:{type:'postgresql',host:'database',port:5432,database:'exhibitos',user:'exhibitos',passwordSecretRef:'database-password'},assets:{type:'filesystem',path:'data/assets'}},secretRefs};
}
/** Mirrors the existing AWS reference adapter input, not an unsupported cloud OED target. */
export function cloudConfiguration(input:CloudInput){
 const name=(v:string)=>typeof v==='string' && /^[a-z][a-z0-9-]{2,47}$/.test(v);
 if(!closed(input,['provider','account_id','region','project','ami_id','instance_type','subnet_id','security_group_ids','key_name','root_gib']) || input.provider!=='aws' || !/^\d{12}$/.test(input.account_id) || !/^[a-z]{2}-[a-z]+-\d$/.test(input.region) || !name(input.project) || !/^ami-[a-f0-9]{8,17}$/.test(input.ami_id) || !['t3.small','t3.medium'].includes(input.instance_type) || !/^subnet-[a-f0-9]{8,17}$/.test(input.subnet_id) || !Array.isArray(input.security_group_ids) || input.security_group_ids.length<1 || input.security_group_ids.length>4 || !input.security_group_ids.every(v=>/^sg-[a-f0-9]{8,17}$/.test(v)) || !name(input.key_name) || !Number.isSafeInteger(input.root_gib) || input.root_gib<20 || input.root_gib>64)return reject();
 return structuredClone(input);
}
export function cloudSummary(input:CloudInput){
 const target=cloudConfiguration(input);
 return {provider:'aws',project:target.project,accountId:target.account_id,region:target.region,pins:{opentofu:'1.12.0',aws:'6.42.0'},changes:[{address:'aws_instance.exhibition',action:'proposed-create',instanceType:target.instance_type,rootGiB:target.root_gib}],cost:{currency:'KRW',monthlyEstimate:null,components:null,source:'operator-estimate',enforced:false,initialAllowance:0},runtimeInstalled:false,healthVerified:false,credentialsIncluded:false,fallback:'generic-ssh'};
}
export function descriptorText(value:ReturnType<typeof deploymentDescriptor>|CloudInput){return JSON.stringify(value,null,2)+'\n';}
