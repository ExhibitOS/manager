// SPDX-License-Identifier: Apache-2.0
import {executeHosted,inspectStaticBundle} from '../vendor/deployment/src/pages-render.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
const fixedCodes=new Set(["HOSTED_API_ORIGIN_INVALID", "HOSTED_API_ORIGIN_UNVERIFIED", "HOSTED_BUSY", "HOSTED_CANONICAL_ORIGIN_CONFLICT", "HOSTED_CONFIGURATION_REQUIRED", "HOSTED_CONNECTION_FAILED", "HOSTED_DEPLOYMENT_BRANCH_CONFLICT", "HOSTED_EXISTING_DATABASE_REQUIRED", "HOSTED_FOLDER_UNAVAILABLE", "HOSTED_KEYCHAIN_REMOVE_FAILED", "HOSTED_KEYCHAIN_STORE_FAILED", "HOSTED_KEYCHAIN_UNAVAILABLE", "HOSTED_LINK_UNAVAILABLE", "HOSTED_OPERATION_UNCERTAIN", "HOSTED_PAGINATION_LIMIT", "HOSTED_PAID_PLAN_REFUSED", "HOSTED_PERMISSION_REQUIRED", "HOSTED_PLATFORM_UNSUPPORTED", "HOSTED_PROJECT_CONFLICT", "HOSTED_RATE_LIMITED", "HOSTED_REDIRECT_REFUSED", "HOSTED_REMOTE_REJECTED", "HOSTED_RENDER_NOT_REQUIRED", "HOSTED_REQUEST_INVALID", "HOSTED_RESOURCE_CONFLICT", "HOSTED_RESPONSE_INVALID", "HOSTED_RESULT_UNCERTAIN", "HOSTED_RUNTIME_UNAVAILABLE","HOSTED_RUNTIME_INPUT_UNCERTAIN","HOSTED_RUNTIME_WAIT_UNCERTAIN","HOSTED_RUNTIME_TIMEOUT_UNCERTAIN","HOSTED_RUNTIME_OUTPUT_UNCERTAIN", "HOSTED_SOURCE_INVALID", "HOSTED_SOURCE_REQUIRED", "HOSTED_STATIC_BUNDLE_CHANGED", "HOSTED_STATIC_BUNDLE_INVALID", "HOSTED_TOKEN_INVALID", "HOSTED_TOKEN_REQUIRED", "HOSTED_URL_UNAVAILABLE", "HOSTED_WORK_INVALID", "HOSTED_WORK_UNAVAILABLE"]);
const stages=new Set(['INPUT','OUTPUT','ACCOUNTS','MEMBERSHIPS','PROJECT_READ','PROJECT_CREATE','UPLOAD_TOKEN','ASSET_CHECK','ASSET_UPLOAD','ASSET_UPSERT','DEPLOY_LIST','DEPLOY_STATUS','DEPLOY_CREATE','BUNDLE_VERIFY','ASSET_PREPARE','DEPLOY_FORM','RENDER_REQUEST']);
const jsDiagnostic=/^HOSTED_JS_(?:INPUT|OUTPUT|ACCOUNTS|MEMBERSHIPS|PROJECT_READ|PROJECT_CREATE|UPLOAD_TOKEN|ASSET_CHECK|ASSET_UPLOAD|ASSET_UPSERT|DEPLOY_LIST|DEPLOY_STATUS|DEPLOY_CREATE|BUNDLE_VERIFY|ASSET_PREPARE|DEPLOY_FORM|RENDER_REQUEST)_(?:TYPE_ERROR|RANGE_ERROR|SYNTAX_ERROR|REFERENCE_ERROR|ERROR|UNKNOWN)$/;
const cfDiagnostic=/^HOSTED_CF_(?:ASSET_CHECK|ASSET_UPSERT|ASSET_UPLOAD|UPLOAD_TOKEN|DEPLOY_STATUS|DEPLOY_CREATE|DEPLOY_LIST|PROJECT_CREATE|PROJECT_READ)_HTTP_(?:2[0-9]{2}|4[0-9]{2})(?:_CODE_[1-9][0-9]{3,7})?$/;
const safe=code=>typeof code==='string'&&(code.startsWith('HOSTED_JS_')?jsDiagnostic.test(code):fixedCodes.has(code)||cfDiagnostic.test(code));
let currentStage='INPUT';
try {
 let bytes=0; const chunks=[];
 for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>131072)throw Error('HOSTED_REQUEST_INVALID');chunks.push(chunk);}
 const input=boundedJSON(Buffer.concat(chunks).toString('utf8'),131072);
 if(!input||Object.keys(input).sort().join(',')!=='request,runtime,tokens')throw Error('HOSTED_REQUEST_INVALID');
 if(input.request?.action==='deploy-api'){currentStage='BUNDLE_VERIFY';await inspectStaticBundle(input.request.sourcePath,{expectedManifestSha256:input.request.expectedManifestSha256});}
 const result=await executeHosted(input.request,{fetch:globalThis.fetch,tokens:input.tokens,onStage:stage=>{if(stages.has(stage))currentStage=stage;},...(input.runtime?{runtime:input.runtime}:{})});
 currentStage='OUTPUT';
 const output=JSON.stringify(result);
 if(Buffer.byteLength(output)>262144)throw Error('HOSTED_RESPONSE_INVALID');
 const secrets=[...Object.values(input.tokens),...(input.runtime?.envVars??[]).filter(v=>['ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'].includes(v.key)).map(v=>v.value)].filter(v=>typeof v==='string'&&v.length);
 const leaks=value=>typeof value==='string'?secrets.some(secret=>value.includes(secret)):Array.isArray(value)?value.some(leaks):value&&typeof value==='object'?Object.entries(value).some(([key,v])=>leaks(key)||leaks(v)):false;
 if(leaks(result))throw Error('HOSTED_RESPONSE_INVALID');
 process.stdout.write(output);
}catch(error){
 const raw=error?.code??error?.message;
 const mapped=raw==='RENDER_CONFIGURATION_REQUIRED'?'HOSTED_CONFIGURATION_REQUIRED':raw==='RENDER_EXISTING_DATABASE_REQUIRED'?'HOSTED_EXISTING_DATABASE_REQUIRED':raw;
 const kind=error instanceof TypeError?'TYPE_ERROR':error instanceof RangeError?'RANGE_ERROR':error instanceof SyntaxError?'SYNTAX_ERROR':error instanceof ReferenceError?'REFERENCE_ERROR':error instanceof Error?'ERROR':'UNKNOWN';
 const errorCode=safe(mapped)?mapped:`HOSTED_JS_${currentStage}_${kind}`;
 // One bounded stdout frame is independent of incidental Node/OS stderr warnings.
 // Never include an exception message, raw exception code, stack or request data.
 process.stdout.write(JSON.stringify({errorCode}));
 process.exitCode=1;
}
