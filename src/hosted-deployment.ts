// SPDX-License-Identifier: Apache-2.0
import {invoke,isTauri} from '@tauri-apps/api/core';
export type HostedProvider='cloudflare'|'render';
export type HostedAccount={id:string;name:string};
export type HostedResource={id:string;name:string;free:boolean;expiresAt?:string};
export type HostedResume={provider:HostedProvider;accountId?:string;workspaceId?:string;source:HostedSource;receipt?:HostedReceipt;phase:string;sourceAvailable:boolean;staticOnly:boolean;apiOrigin?:string;staticReceipt?:HostedReceipt|null;apiReceipt?:HostedReceipt|null};
export type HostedSource={title:string;publicationId:string;projectName:string};
export type HostedRequest={provider:HostedProvider;action:'accounts'|'services'|'reconcile'|'deploy-static'|'deploy-api'|'status';accountId?:string;workspaceId?:string;projectName?:string;sourcePath?:string;deploymentId?:string;serviceId?:string;postgresId?:string;staticOnly?:boolean;publicUploadApproved?:boolean;costReviewed?:boolean;apiRequested?:boolean};
export type HostedReceipt={provider:HostedProvider;action:HostedRequest['action'];state:'ready'|'deployed'|'deploying'|'failed'|'uncertain'|'blocked'|'needs-configuration';accounts?:HostedAccount[];services?:HostedResource[];postgres?:HostedResource[];databaseState?:'available'|'creating'|'unavailable';projectName?:string;deploymentId?:string;serviceId?:string;postgresId?:string;url?:string;expiresAt?:string;code?:string};
const record=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
const text=(v:unknown,max=4096):v is string=>typeof v==='string'&&v.length>0&&v.length<=max&&!/[\u0000-\u001f\u007f]/.test(v);
export function hostedUrl(value:unknown):value is string{
 if(!text(value,2048))return false;
 try{const u=new URL(value);return u.protocol==='https:'&&!u.username&&!u.password&&!u.port&&!u.search&&!u.hash&&(u.hostname.endsWith('.pages.dev')||u.hostname.endsWith('.onrender.com'));}catch{return false;}
}
export function parseHostedReceipt(value:unknown):HostedReceipt{
 if(!record(value)||!['cloudflare','render'].includes(String(value.provider))||!['accounts','services','reconcile','deploy-static','deploy-api','status'].includes(String(value.action))||!['ready','deployed','deploying','failed','uncertain','blocked','needs-configuration'].includes(String(value.state)))throw Error('HOSTED_RESPONSE_INVALID');
 const fields=new Set(['provider','action','state','accounts','services','postgres','databaseState','projectName','deploymentId','serviceId','postgresId','url','expiresAt','code']);
 if(Object.keys(value).some(k=>!fields.has(k)))throw Error('HOSTED_RESPONSE_INVALID');
 for(const key of ['projectName','deploymentId','serviceId','postgresId','code'])if(value[key]!==undefined&&!text(value[key],256))throw Error('HOSTED_RESPONSE_INVALID');
 if(value.url!==undefined&&!hostedUrl(value.url))throw Error('HOSTED_RESPONSE_INVALID');
 if(value.expiresAt!==undefined&&(!text(value.expiresAt,64)||!Number.isFinite(Date.parse(value.expiresAt))))throw Error('HOSTED_RESPONSE_INVALID');
 if(value.accounts!==undefined&&(!Array.isArray(value.accounts)||value.accounts.length>200||value.accounts.some(a=>!record(a)||Object.keys(a).sort().join(',')!=='id,name'||!text(a.id,128)||!text(a.name,256))))throw Error('HOSTED_RESPONSE_INVALID');
 for(const key of ['services','postgres'])if(value[key]!==undefined&&(!Array.isArray(value[key])||value[key].length>200||value[key].some((a:unknown)=>!record(a)||Object.keys(a).some(k=>!['id','name','free','expiresAt'].includes(k))||!text(a.id,128)||!text(a.name,256)||typeof a.free!=='boolean'||(a.expiresAt!==undefined&&(!text(a.expiresAt,64)||!Number.isFinite(Date.parse(a.expiresAt)))))))throw Error('HOSTED_RESPONSE_INVALID');
 if(value.databaseState!==undefined&&!['available','creating','unavailable'].includes(String(value.databaseState)))throw Error('HOSTED_RESPONSE_INVALID');
 if(value.action==='accounts'&&!Array.isArray(value.accounts))throw Error('HOSTED_RESPONSE_INVALID');
 return value as HostedReceipt;
}
export const hostedClient={
 native:()=>isTauri(),
 resume:()=>invoke<HostedResume|null>('manager_hosted_resume'),
 resumeRemote:()=>invoke<HostedResume|null>('manager_hosted_resume_remote'),
 connected:(provider:HostedProvider)=>invoke<boolean>('manager_hosted_connected',{input:{provider}}),
 store:(provider:HostedProvider,token:string)=>invoke<boolean>('manager_hosted_store',{input:{provider,token}}),
 disconnect:(provider:HostedProvider)=>invoke<boolean>('manager_hosted_disconnect',{input:{provider}}),
 openUrl:(url:string)=>invoke<void>('manager_hosted_open_url',{input:{url}}),
 source:()=>invoke<HostedSource|null>('manager_hosted_choose_source'),
 link:(link:'cloudflare-signup'|'cloudflare-token'|'render-signup'|'render-token')=>invoke<void>('manager_hosted_link',{link}),
 operation:async(input:HostedRequest)=>{const receipt=parseHostedReceipt(await invoke<unknown>('manager_hosted',{input}));if(receipt.provider!==input.provider||receipt.action!==input.action)throw Error('HOSTED_RESPONSE_INVALID');return receipt;}
};
export function hostedFailure(value:unknown):string{
 const code=record(value)&&typeof value.code==='string'?value.code:value instanceof Error?value.message:'';
 const js=/^HOSTED_JS_(?:INPUT|OUTPUT|ACCOUNTS|MEMBERSHIPS|PROJECT_READ|PROJECT_CREATE|UPLOAD_TOKEN|ASSET_CHECK|ASSET_UPLOAD|ASSET_UPSERT|DEPLOY_LIST|DEPLOY_STATUS|DEPLOY_CREATE|BUNDLE_VERIFY|ASSET_PREPARE|DEPLOY_FORM|RENDER_REQUEST)_(?:TYPE_ERROR|RANGE_ERROR|SYNTAX_ERROR|REFERENCE_ERROR|ERROR|UNKNOWN)$/;
 if(code.startsWith('HOSTED_JS_'))return js.test(code)?code:'HOSTED_OPERATION_UNCERTAIN';
 return (new Set(["HOSTED_API_ORIGIN_INVALID", "HOSTED_API_ORIGIN_UNVERIFIED", "HOSTED_BUSY", "HOSTED_CANONICAL_ORIGIN_CONFLICT", "HOSTED_CONFIGURATION_REQUIRED", "HOSTED_CONNECTION_FAILED", "HOSTED_DEPLOYMENT_BRANCH_CONFLICT", "HOSTED_EXISTING_DATABASE_REQUIRED", "HOSTED_FOLDER_UNAVAILABLE", "HOSTED_KEYCHAIN_REMOVE_FAILED", "HOSTED_KEYCHAIN_STORE_FAILED", "HOSTED_KEYCHAIN_UNAVAILABLE", "HOSTED_LINK_UNAVAILABLE", "HOSTED_OPERATION_UNCERTAIN", "HOSTED_PAGINATION_LIMIT", "HOSTED_PAID_PLAN_REFUSED", "HOSTED_PERMISSION_REQUIRED", "HOSTED_PLATFORM_UNSUPPORTED", "HOSTED_PROJECT_CONFLICT", "HOSTED_RATE_LIMITED", "HOSTED_REDIRECT_REFUSED", "HOSTED_REMOTE_REJECTED", "HOSTED_RENDER_NOT_REQUIRED", "HOSTED_REQUEST_INVALID", "HOSTED_RESOURCE_CONFLICT", "HOSTED_RESPONSE_INVALID", "HOSTED_RESULT_UNCERTAIN", "HOSTED_RUNTIME_UNAVAILABLE","HOSTED_RUNTIME_INPUT_UNCERTAIN","HOSTED_RUNTIME_WAIT_UNCERTAIN","HOSTED_RUNTIME_TIMEOUT_UNCERTAIN","HOSTED_RUNTIME_OUTPUT_UNCERTAIN", "HOSTED_SOURCE_INVALID", "HOSTED_SOURCE_REQUIRED", "HOSTED_STATIC_BUNDLE_CHANGED", "HOSTED_STATIC_BUNDLE_INVALID", "HOSTED_TOKEN_INVALID", "HOSTED_TOKEN_REQUIRED", "HOSTED_URL_UNAVAILABLE", "HOSTED_WORK_INVALID", "HOSTED_WORK_UNAVAILABLE"]).has(code)||/^HOSTED_CF_(?:ASSET_CHECK|ASSET_UPSERT|ASSET_UPLOAD|UPLOAD_TOKEN|DEPLOY_STATUS|DEPLOY_CREATE|DEPLOY_LIST|PROJECT_CREATE|PROJECT_READ)_HTTP_(?:2[0-9]{2}|4[0-9]{2})(?:_CODE_[1-9][0-9]{3,7})?$/.test(code))?code:'HOSTED_OPERATION_UNCERTAIN';
}

export function hostedUncertain(value:unknown):boolean{
 const code=hostedFailure(value);
 return code.includes('UNCERTAIN')||code.startsWith('HOSTED_JS_')||/^HOSTED_CF_.*_HTTP_2/.test(code)||code==='HOSTED_RESPONSE_INVALID';
}
