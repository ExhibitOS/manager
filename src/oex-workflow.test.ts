// SPDX-License-Identifier: Apache-2.0
import {beforeEach,describe,expect,it,vi} from 'vitest';
const invoke=vi.hoisted(()=>vi.fn());
vi.mock('@tauri-apps/api/core',()=>({invoke,isTauri:()=>true}));
const id='56789012-1234-1234-1234-123456789012',token='45678901-1234-1234-1234-123456789012';
const context={selectionToken:token,activeId:id,mode:'override',installations:[{id,kind:'override',createdAt:1,path:'/private/tmp/oex-synthetic',available:true}],errorCode:null};
beforeEach(()=>{vi.resetModules();invoke.mockReset();});
describe('Selected installation OEX native handoff',()=>{
 it('passes only the current selection token, never a URL, path or login token',async()=>{
  invoke.mockResolvedValueOnce(context).mockResolvedValueOnce(undefined);
  const {client}=await import('./lifecycle');await client.openOex();
  expect(invoke.mock.calls).toEqual([['manager_installations'],['manager_open_oex',{selectionToken:token}]]);
 });
 it('propagates native readiness refusal instead of reporting an OEX job complete',async()=>{
  invoke.mockResolvedValueOnce(context).mockRejectedValueOnce({code:'READINESS_UNAVAILABLE',guidance:'먼저 시작하세요.'});
  const {client}=await import('./lifecycle');await expect(client.openOex()).rejects.toMatchObject({code:'READINESS_UNAVAILABLE'});
 });
 it('refuses malformed selection before requesting the native browser handoff',async()=>{
  invoke.mockResolvedValueOnce({...context,selectionToken:'https://remote.example/'});
  const {client}=await import('./lifecycle');await expect(client.openOex()).rejects.toThrow('MANAGER_PROTOCOL');
  expect(invoke).toHaveBeenCalledTimes(1);
 });
});
