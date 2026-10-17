// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseHostedReceipt,parseHostedResume,hostedUrl,hostedFailure,hostedUncertain} from './hosted-deployment';
describe('hosted public receipt boundary',()=>{
 it('keeps provider completion separate from dynamic application readiness and optional expiry',()=>{
  const r=parseHostedReceipt({provider:'render',action:'status',state:'deployed',databaseState:'available',serviceId:'srv-synthetic'});
  expect(r.state).toBe('deployed');expect(r.expiresAt).toBeUndefined();
  expect(parseHostedReceipt({provider:'render',action:'services',state:'ready',services:[{id:'srv-a',name:'API',free:true}],postgres:[{id:'pg-a',name:'Data',free:true,expiresAt:'2026-11-17T00:00:00Z'}]}).services).toHaveLength(1);
 });
 it('rejects credential fields raw server diagnostics malformed accounts and untrusted URLs',()=>{
  const base={provider:'cloudflare',action:'accounts',state:'ready',accounts:[{id:'account-a',name:'My account'}]};
  expect(parseHostedReceipt(base).accounts).toHaveLength(1);
  for(const bad of [{...base,token:'synthetic-token'},{...base,accounts:[{id:'a',name:'A',raw:'private'}]},{...base,url:'https://attacker.invalid/'},{...base,expiresAt:'never'}])expect(()=>parseHostedReceipt(bad)).toThrow('HOSTED_RESPONSE_INVALID');
  expect(hostedUrl('https://my-show.pages.dev')).toBe(true);expect(hostedUrl('https://user:password@my-show.pages.dev')).toBe(false);expect(hostedUrl('https://my-show.pages.dev/?token=secret')).toBe(false);
 });
 it('keeps all bounded 2xx diagnostics and only fixed JavaScript stage/class pairs',()=>{
  for(const code of ['HOSTED_CF_ASSET_UPLOAD_HTTP_204','HOSTED_JS_DEPLOY_FORM_TYPE_ERROR','HOSTED_JS_ASSET_PREPARE_ERROR','HOSTED_RUNTIME_INPUT_UNCERTAIN','HOSTED_RUNTIME_WAIT_UNCERTAIN','HOSTED_RUNTIME_TIMEOUT_UNCERTAIN','HOSTED_RUNTIME_OUTPUT_UNCERTAIN'])expect(hostedFailure({code})).toBe(code);
  for(const code of ['HOSTED_UNCLASSIFIED_EXCEPTION','HOSTED_JS_PRIVATE_ERROR','HOSTED_JS_DEPLOY_FORM_ERR_INVALID_STATE','HOSTED_CF_ASSET_UPLOAD_HTTP_204_SECRET'])expect(hostedFailure({code})).toBe('HOSTED_OPERATION_UNCERTAIN');
 });
 it('keeps unknown JS 2xx envelopes native failures and malformed response frames uncertain',()=>{
  for(const code of ['HOSTED_JS_DEPLOY_FORM_ERROR','HOSTED_CF_ASSET_UPLOAD_HTTP_204','HOSTED_RUNTIME_TIMEOUT_UNCERTAIN','HOSTED_RESPONSE_INVALID'])expect(hostedUncertain({code})).toBe(true);
  expect(hostedUncertain({code:'HOSTED_CF_DEPLOY_LIST_HTTP_400_CODE_8000024'})).toBe(false);
 });
 it('only shows fixed local error codes, never original exception text',()=>{
  expect(hostedFailure({code:'HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_10000'})).toBe('HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_10000');
  expect(hostedFailure({code:'HOSTED_CF_UNKNOWN_HTTP_404_CODE_10000'})).toBe('HOSTED_OPERATION_UNCERTAIN');
  expect(hostedFailure({code:'HOSTED_CF_ASSET_CHECK_HTTP_404_CODE_secret'})).toBe('HOSTED_OPERATION_UNCERTAIN');
  expect(hostedFailure(Error('Bearer synthetic-private-token'))).toBe('HOSTED_OPERATION_UNCERTAIN');
  expect(hostedFailure({code:'HOSTED_TOKEN_REQUIRED',raw:'synthetic-private-token'})).toBe('HOSTED_TOKEN_REQUIRED');
 });
});

describe('saved hosted work rendering boundary',()=>{
 const source={title:'Saved exhibition',publicationId:'publication-a',projectName:'show-a',revisionSha256:'a'.repeat(64),indexSha256:'b'.repeat(64)};
 const work={provider:'cloudflare',accountId:null,workspaceId:null,source,receipt:null,staticReceipt:null,apiReceipt:null,phase:'uncertain',sourceAvailable:false,staticOnly:true};
 it('retains old unavailable-source metadata and null optional receipts without another read',()=>{expect(parseHostedResume(null)).toBeNull();expect(parseHostedResume(work)).toEqual(work);});
 it('refuses malformed saved work before it can break the entire Manager screen',()=>{for(const invalid of [undefined,[],{}, {...work,source:null},{...work,source:[]},{...work,source:{...source,title:4}},{...work,staticOnly:'true'},{...work,receipt:[]}])expect(()=>parseHostedResume(invalid)).toThrow('HOSTED_RESPONSE_INVALID');});
 it('preserves observed receipts but rejects a crossed provider receipt',()=>{const receipt={provider:'cloudflare',action:'status',state:'ready',url:'https://show-a.pages.dev'};expect(parseHostedResume({...work,staticReceipt:receipt})?.staticReceipt).toEqual(receipt);expect(()=>parseHostedResume({...work,apiReceipt:receipt})).toThrow('HOSTED_RESPONSE_INVALID');});
});
