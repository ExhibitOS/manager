// SPDX-License-Identifier: Apache-2.0
import {planSsh,applySsh,verifySshEndpoint,maintenanceSsh,rollbackSsh,runSsh} from '../vendor/deployment/src/generic-ssh.mjs';
import {projectMaintenance} from '../vendor/deployment/src/ssh-maintenance.mjs';
import {realpathSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const basic=['plan','status','logs','verify','install','start','stop','restart','retry'],maintenance=['backup','backup-jobs','update','update-status'];
const closed=(v,k)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===[...k].sort().join(',');
const fail=code=>{throw Object.assign(Error(code),{code});};
const hash=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v),uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v),code=v=>v===null||(typeof v==='string'&&/^[A-Z0-9_]{1,64}$/.test(v));
export function project(action,r,input){
 if(maintenance.includes(action))return projectMaintenance(action,r,input?.operationId);
 if(r?.version!==1)fail('SSH_RESPONSE_INVALID');
 if(action==='plan'){
  if(!closed(r,['version','targetDigest','oedDigest','bindingDigest','manualDNS','tlsVerified','firewallPolicyObserved','firewallVerified','healthVerified','credentialsIncluded'])||![r.targetDigest,r.oedDigest,r.bindingDigest].every(hash)||!closed(r.manualDNS,['hostname','target'])||![r.manualDNS.hostname,r.manualDNS.target].every(v=>typeof v==='string'&&v.length<=253&&/^[a-zA-Z0-9.:-]+$/.test(v))||r.credentialsIncluded!==false||r.tlsVerified!==false||r.firewallVerified!==false||r.healthVerified!==false||typeof r.firewallPolicyObserved!=='boolean')fail('SSH_RESPONSE_INVALID');
  return {version:1,action,prepared:true,credentialsIncluded:false,healthVerified:false,tlsVerified:false,firewallVerified:false,manualDNS:true};
 }
 if(action==='verify'){if(!closed(r,['version','dnsVerified','tlsVerified','healthVerified','firewallVerified','manualDNS'])||r.dnsVerified!==true||r.tlsVerified!==true||r.healthVerified!==true||r.firewallVerified!==false||r.manualDNS!==true)fail('SSH_RESPONSE_INVALID');return {version:1,action,...r};}
 if(action==='status'){if(!closed(r,['version','state','ready','healthVerified'])||typeof r.state!=='string'||!/^[a-z_-]{1,64}$/.test(r.state)||typeof r.ready!=='boolean'||r.healthVerified!==false)fail('SSH_RESPONSE_INVALID');return {version:1,action,...r};}
 if(action==='logs'){if(!closed(r,['version','entries','rawLogsIncluded'])||r.rawLogsIncluded!==false||!Array.isArray(r.entries)||r.entries.length>50||r.entries.some(e=>!closed(e,['at','code'])||!Number.isSafeInteger(e.at)||typeof e.code!=='string'||!code(e.code)))fail('SSH_RESPONSE_INVALID');return {version:1,action,...r};}
 if(action==='rollback'){
  if(!closed(r,['version','action','state','operationId','candidateId','authorityGeneration','restorationVerified','activationVerified','selectionVerified','healthVerified','preservedVolumes','tlsVerified','firewallVerified'])||r.action!==action||r.state!=='completed'||r.operationId!==input.operationId||!uuid(r.candidateId)||!Number.isSafeInteger(r.authorityGeneration)||r.authorityGeneration<=input.expectedGeneration||!['restorationVerified','activationVerified','selectionVerified','healthVerified','preservedVolumes'].every(k=>r[k]===true)||r.tlsVerified!==false||r.firewallVerified!==false)fail('SSH_RESPONSE_INVALID');return structuredClone(r);
 }
 const keys=r.idempotent===true?['version','action','state','idempotent','preservedVolumes','healthVerified']:['version','action','state','jobId','code','preservedVolumes','healthVerified'];
 if(!closed(r,keys)||r.action!==action||!['completed','failed'].includes(r.state)||r.preservedVolumes!==true||r.healthVerified!==false||(!r.idempotent&&(!uuid(r.jobId)||!code(r.code))))fail('SSH_RESPONSE_INVALID');return structuredClone(r);
}
export async function executeRemote(request,adapter={planSsh,applySsh,verifySshEndpoint,maintenanceSsh,rollbackSsh,runSsh},signal){
 if(!closed(request,['requestId','action','oed','connection','consent','maintenance'])||!uuid(request.requestId)||![...basic,...maintenance,'rollback'].includes(request.action)||!closed(request.consent,['preserveVolumes','existingImagesOnly','externalWritersQuiesced','appsClosed'])||Object.values(request.consent).some(v=>typeof v!=='boolean'))fail('SSH_REQUEST_INVALID');
 if(!request.consent.preserveVolumes||!request.consent.existingImagesOnly)fail('SSH_CONSENT_REQUIRED');
 const action=request.action;
 if(basic.includes(action)&&request.maintenance!==null)fail('SSH_REQUEST_INVALID');
 if(['backup','update','rollback'].includes(action)&&(!request.consent.externalWritersQuiesced||!request.consent.appsClosed))fail('SSH_CONSENT_REQUIRED');
 if(signal?.aborted)fail('SSH_CANCELLED');
 let result;
 if(action==='verify')result=await adapter.verifySshEndpoint(request.oed,request.connection);
 else{
  // Bind the cancellation signal to every subsequent use of this observed plan,
  // including the existing applySsh API whose options do not carry a signal.
  const run=(...args)=>adapter.runSsh(args[0],args[1],args[2],{...args[3],signal});
  const plan=await adapter.planSsh(request.oed,request.connection,{signal,run});
  if(action==='plan')result=plan;
  else if(maintenance.includes(action))result=await adapter.maintenanceSsh(plan,{action,input:request.maintenance,preserveVolumes:true,externalWritersQuiesced:request.consent.externalWritersQuiesced,appsClosed:request.consent.appsClosed,signal});
  else if(action==='rollback')result=await adapter.rollbackSsh(plan,{rollback:request.maintenance,preserveVolumes:true,externalWritersQuiesced:request.consent.externalWritersQuiesced,appsClosed:request.consent.appsClosed,signal});
  else result=await adapter.applySsh(plan,{action,preserveVolumes:true,existingImagesOnly:true});
 }
 if(signal?.aborted)fail('SSH_CANCELLED_UNCERTAIN');return project(action,result,request.maintenance);
}
export async function main(){
 const abort=new AbortController(),cancel=()=>abort.abort();process.on('SIGTERM',cancel);process.on('SIGINT',cancel);
 try{let text='',bytes=0;for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>131072)fail('SSH_REQUEST_TOO_LARGE');text+=chunk;}
  const result=await executeRemote(boundedJSON(text,131072),undefined,abort.signal);const output=JSON.stringify(result);if(Buffer.byteLength(output)>262144)fail('SSH_RESPONSE_TOO_LARGE');process.stdout.write(output+'\n');
 }catch(e){const value=e.code??e.message??'';process.stderr.write((typeof value==='string'&&/^SSH_[A-Z_]{1,60}$/.test(value)?value:'SSH_OPERATION_REFUSED')+'\n');process.exitCode=1;}
 finally{process.off('SIGTERM',cancel);process.off('SIGINT',cancel);}
}
const direct=process.argv[1]&&(()=>{try{return fileURLToPath(import.meta.url)===realpathSync(process.argv[1]);}catch{return false;}})();
if(direct)await main();
