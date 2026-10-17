// SPDX-License-Identifier: Apache-2.0
import {fileURLToPath} from 'node:url';
import {listLocalPublications} from './publication-client.mjs';
const allowed=new Set(['LOCAL_CMS_AUTH_REQUIRED','LOCAL_CMS_PERMISSION_REQUIRED','LOCAL_CMS_RESPONSE_INVALID','LOCAL_CMS_UNAVAILABLE','LOCAL_CMS_REQUEST_INVALID','LOCAL_CMS_SOURCE_CHANGED','LOCAL_CMS_EXPORT_FAILED']);
try {
 let bytes=0;const chunks=[];
 for await(const chunk of process.stdin){bytes+=chunk.length;if(bytes>16384)throw Error('LOCAL_CMS_REQUEST_INVALID');chunks.push(chunk);}
 const input=JSON.parse(Buffer.concat(chunks).toString('utf8'));
 let result;
 if(input?.action==='catalog' && Object.keys(input).sort().join(',')==='action,origin,password,subject,tenantId'){
  result=await listLocalPublications(input);
 }else if(input?.action==='export' && Object.keys(input).sort().join(',')==='action,expectedRevisionSha256,origin,output,publicationId'){
  if(!/^[a-f0-9]{64}$/.test(input.expectedRevisionSha256))throw Error('LOCAL_CMS_REQUEST_INVALID');
  const {exportStaticPublication}=await import('../publication-runtime/generated/exporter.mjs');
  const receipt=await exportStaticPublication({origin:input.origin,publicationId:input.publicationId,output:input.output,runtimeRoot:fileURLToPath(new URL('../publication-runtime/generated/viewer/',import.meta.url))});
  if(receipt.revisionSha256!==input.expectedRevisionSha256)throw Error('LOCAL_CMS_SOURCE_CHANGED');
  result={publicationId:receipt.publicationId,revisionSha256:receipt.revisionSha256,manifestSha256:receipt.manifestSha256};
 }else throw Error('LOCAL_CMS_REQUEST_INVALID');
 const output=JSON.stringify(result);
 if(Buffer.byteLength(output)>262144 || (input.password && output.includes(input.password)))throw Error('LOCAL_CMS_RESPONSE_INVALID');
 process.stdout.write(output);
}catch(e){const mapped={LOCAL_CMS_LOGIN:'LOCAL_CMS_AUTH_REQUIRED',LOCAL_CMS_FORBIDDEN:'LOCAL_CMS_PERMISSION_REQUIRED',LOCAL_CMS_TENANT:'LOCAL_CMS_PERMISSION_REQUIRED',LOCAL_CMS_CATALOG:'LOCAL_CMS_RESPONSE_INVALID',LOCAL_CMS_SESSION:'LOCAL_CMS_AUTH_REQUIRED',LOCAL_CMS_LOGOUT:'LOCAL_CMS_UNAVAILABLE',LOCAL_CMS_REQUEST:'LOCAL_CMS_UNAVAILABLE',LOCAL_CMS_ORIGIN:'LOCAL_CMS_REQUEST_INVALID',LOCAL_CMS_INPUT:'LOCAL_CMS_REQUEST_INVALID'}[e?.message]??e?.message;const errorCode=allowed.has(mapped)?mapped:'LOCAL_CMS_EXPORT_FAILED';process.stdout.write(JSON.stringify({errorCode}));process.exitCode=1;}
