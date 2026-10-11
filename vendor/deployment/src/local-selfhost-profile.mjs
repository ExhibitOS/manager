// Copyright 2026 ExhibitOS contributors. SPDX-License-Identifier: Apache-2.0
import {isDeepStrictEqual as equal} from 'node:util';
import {boundedJSON} from './bounded-json.mjs';
const fail=code=>{throw Object.assign(new Error(code),{code});};
const closed=(v,keys)=>v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===keys.sort().join(',');
const uuid=v=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(v);
const image=v=>typeof v==='string'&&v.length<=512&&/^[a-z0-9][a-z0-9./:_-]+@sha256:[a-f0-9]{64}$/.test(v);
const hash=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
function utc(v){if(typeof v!=='string'||!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(v)||!Number.isFinite(Date.parse(v)))return false;return new Date(v).toISOString().replace('.000Z','Z')===v.replace('.000Z','Z');}
/** Closed local subset of public OED v1, then exact two-service packaging contract. Hash approval never substitutes for document validity. */
export function validateLocalSelfhostProfile(o,m,c,composeDigest){
 if(!closed(o,['schemaVersion','kind','id','createdAt','target','runtime','storage','secretRefs'])||o.schemaVersion!=='1.0.0-draft.1'||o.kind!=='deployment'||!uuid(o.id)||!utc(o.createdAt)||!closed(o.target,['type'])||o.target.type!=='local-compose'||!closed(o.runtime,['image','httpPort','replicas'])||!image(o.runtime.image)||o.runtime.httpPort!==13200||o.runtime.replicas!==1)fail('SELFHOST_PROFILE_UNSUPPORTED');
 const db=o.storage?.metadata,assets=o.storage?.assets,ref=o.secretRefs?.[0];
 if(!closed(o.storage,['metadata','assets'])||!closed(db,['type','host','port','database','user','passwordSecretRef'])||db.type!=='postgresql'||db.host!=='database'||db.port!==5432||db.database!=='exhibitos'||db.user!=='exhibitos'||typeof db.passwordSecretRef!=='string'||! /^[a-z][a-z0-9-]{0,63}$/.test(db.passwordSecretRef)||!closed(assets,['type','path'])||assets.type!=='filesystem'||assets.path!=='data/assets'||!Array.isArray(o.secretRefs)||o.secretRefs.length!==1||!closed(ref,['id','source','variable'])||ref.id!==db.passwordSecretRef||ref.source!=='environment'||! /^EXHIBITOS_[A-Z0-9_]{1,64}$/.test(ref.variable))fail('SELFHOST_PROFILE_UNSUPPORTED');
 const network=m.explicitLocalNetwork;
 if(network!==undefined&&(!closed(network,['mode','subnet','gateway'])||network.mode!=='explicit-rfc1918-v1'||typeof network.subnet!=='string'||!/^10\.240\.\d{1,3}\.\d{1,3}\/28$/.test(network.subnet)))fail('SELFHOST_NETWORK_POLICY_INVALID');
 if(network!==undefined){const parts=network.subnet.slice(0,-3).split('.').map(Number);if(parts.some((n,i)=>n>255||String(n)!==network.subnet.slice(0,-3).split('.')[i])||parts[3]%16!==0||network.gateway!==`10.240.${parts[2]}.${parts[3]+1}`)fail('SELFHOST_NETWORK_POLICY_INVALID');}
 if(!closed(m,['schemaVersion','bundleId','version','protocolVersion','composeSha256','preferredEngine','projectName','services','images','ports','openUrl','readinessUrl','minimumFreeBytes',...(network===undefined?[]:['explicitLocalNetwork'])])||m.schemaVersion!=='1.0.0-draft.1'||!uuid(m.bundleId)||m.version!=='0.1.0'||m.protocolVersion!=='1'||m.preferredEngine!=='docker'||m.projectName!==`exhibitos-${m.bundleId}`||m.composeSha256!==composeDigest||!hash(m.composeSha256)||!equal(m.services,['platform','database'])||!equal(m.ports,[13200])||m.openUrl!=='http://127.0.0.1:13200'||m.readinessUrl!=='http://127.0.0.1:13200/api/v1/readiness'||m.minimumFreeBytes!==3*1024*1024*1024||!Array.isArray(m.images)||m.images.length!==2)fail('SELFHOST_BUNDLE_MISMATCH');
 for(const [i,entry] of m.images.entries()){
  if(!(closed(entry,['reference'])||closed(entry,['reference','archive']))||!image(entry.reference))fail('SELFHOST_BUNDLE_MISMATCH');
  if(entry.archive!==undefined&&entry.archive!==null){const a=entry.archive;if(i!==0||!closed(a,['path','sha256','bytes'])||a.path!=='platform-image.tar'||!hash(a.sha256)||!Number.isSafeInteger(a.bytes)||a.bytes<1||a.bytes>8*1024*1024*1024)fail('SELFHOST_BUNDLE_MISMATCH');}
 }
 if(m.images[0].reference!==o.runtime.image)fail('SELFHOST_BUNDLE_MISMATCH');
 const labels={'com.exhibitos.bundle':m.bundleId,'com.exhibitos.project':m.projectName,'com.exhibitos.schema':'1.0.0-draft.1'};
 const expected={name:m.projectName,services:{
  database:{image:m.images[1].reference,environment:{POSTGRES_PASSWORD:'${POSTGRES_PASSWORD:?required}',POSTGRES_DB:'exhibitos',POSTGRES_USER:'exhibitos'},volumes:['database:/var/lib/postgresql'],labels,healthcheck:{test:['CMD-SHELL','pg_isready -U exhibitos -d exhibitos'],interval:'5s',timeout:'3s',retries:30},restart:'unless-stopped'},
  platform:{image:m.images[0].reference,pull_policy:'never',env_file:['../runtime.env'],environment:{NODE_ENV:'production',EXHIBITOS_PORT:'${EXHIBITOS_PORT:-13200}',AUTH_ORIGIN:'http://127.0.0.1:${EXHIBITOS_PORT:-13200}',BLOB_ROOT:'/data/blobs',CONFIG_ROOT:'/data/config'},ports:['127.0.0.1:${EXHIBITOS_PORT:-13200}:8080'],volumes:['objects:/data/blobs','configuration:/data/config'],labels,depends_on:{database:{condition:'service_healthy'}},read_only:true,tmpfs:['/tmp:rw,nosuid,nodev,size=256m,mode=1777'],cap_drop:['ALL'],security_opt:['no-new-privileges:true'],restart:'unless-stopped',stop_grace_period:'30s'}
 },volumes:{database:{labels},objects:{labels},configuration:{labels}},networks:{default:{labels}}};
 if(network!==undefined)expected.networks.default.ipam={config:[{subnet:network.subnet,gateway:network.gateway}]};
 if(!equal(c,boundedJSON(JSON.stringify(expected),65536)))fail('SELFHOST_COMPOSE_PROFILE_MISMATCH');
}
