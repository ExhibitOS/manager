// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseHostedReceipt,hostedUrl,hostedFailure} from './hosted-deployment';
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
 it('only shows fixed local error codes, never original exception text',()=>{
  expect(hostedFailure(Error('Bearer synthetic-private-token'))).toBe('HOSTED_OPERATION_UNCERTAIN');
  expect(hostedFailure({code:'HOSTED_TOKEN_REQUIRED',raw:'synthetic-private-token'})).toBe('HOSTED_TOKEN_REQUIRED');
 });
});
