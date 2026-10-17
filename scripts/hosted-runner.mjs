// SPDX-License-Identifier: Apache-2.0
import {executeHosted,inspectStaticBundle} from '../vendor/deployment/src/pages-render.mjs';
import {boundedJSON} from '../vendor/deployment/src/bounded-json.mjs';
try {
 let bytes=0; const chunks=[];
 for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>131072)throw Error('HOSTED_REQUEST_INVALID');chunks.push(chunk);}
 const input=boundedJSON(Buffer.concat(chunks).toString('utf8'),131072);
 if(!input||Object.keys(input).sort().join(',')!=='request,runtime,tokens')throw Error('HOSTED_REQUEST_INVALID');
 if(input.request?.action==='deploy-api')await inspectStaticBundle(input.request.sourcePath,{expectedManifestSha256:input.request.expectedManifestSha256});
 const result=await executeHosted(input.request,{fetch:globalThis.fetch,tokens:input.tokens,...(input.runtime?{runtime:input.runtime}:{})});
 const output=JSON.stringify(result);
 if(Buffer.byteLength(output)>262144)throw Error('HOSTED_RESPONSE_INVALID');
 const secrets=[...Object.values(input.tokens),...(input.runtime?.envVars??[]).filter(v=>['ADMIN_PASSWORD','FREEZE_SIGNING_KEY_BASE64'].includes(v.key)).map(v=>v.value)].filter(v=>typeof v==='string'&&v.length);
 const leaks=value=>typeof value==='string'?secrets.some(secret=>value.includes(secret)):Array.isArray(value)?value.some(leaks):value&&typeof value==='object'?Object.entries(value).some(([key,v])=>leaks(key)||leaks(v)):false;
 if(leaks(result))throw Error('HOSTED_RESPONSE_INVALID');
 process.stdout.write(output);
}catch(error){const raw=error?.code??error?.message;const code=raw==='RENDER_CONFIGURATION_REQUIRED'?'HOSTED_CONFIGURATION_REQUIRED':raw==='RENDER_EXISTING_DATABASE_REQUIRED'?'HOSTED_EXISTING_DATABASE_REQUIRED':raw;process.stderr.write(typeof code==='string'&&/^HOSTED_[A-Z_]{1,72}$/.test(code)?code:'HOSTED_OPERATION_UNCERTAIN');process.exitCode=1;}
