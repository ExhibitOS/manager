// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parseInstallationContext,managerError} from './lifecycle';
const id='56789012-1234-1234-1234-123456789012';
const token='45678901-1234-1234-1234-123456789012';
const context=(path:string)=>({selectionToken:token,activeId:id,mode:'override',installations:[{id,kind:'override',createdAt:1,path,available:true}],errorCode:null});
describe('Native canonical Windows installation paths',()=>{
 it('accepts the extended-length drive path emitted by the private Windows root',()=>{
  const path=String.raw`\\?\C:\Users\Jung\AppData\Local\ExhibitOS-GuiCheck-synthetic`;
  expect(parseInstallationContext(context(path)).installations[0]?.path).toBe(path);
 });
 it('still accepts ordinary absolute drive and Unix paths, and rejects non-file namespaces',()=>{
  for(const path of [String.raw`C:\Users\Jung\ExhibitOS`, 'C:/Users/Jung/ExhibitOS','/private/tmp/synthetic'])expect(parseInstallationContext(context(path)).installations[0]?.path).toBe(path);
  for(const path of [String.raw`\\.\PhysicalDrive0`,String.raw`\\?\GLOBALROOT\Device\HarddiskVolume1`,String.raw`\\?\UNC\server\share`,String.raw`C:relative`,String.raw`\\?\C:relative`,'https://example.com/profile','relative','C:\\profile\nSECRET=private'])expect(()=>parseInstallationContext(context(path))).toThrow('MANAGER_PROTOCOL');
 });
 it('binds managed Windows roots through normalized separators while preserving exact display paths',()=>{
  const profile=String.raw`\\?\C:\Users\Jung\AppData\Local\ExhibitOS`;
  const other='67890123-1234-1234-1234-123456789012';
  const managed={...context(profile),mode:'managed',installations:[{id,kind:'default',createdAt:1,path:profile+String.raw`\local-runtime`,available:true},{id:other,kind:'recovery',createdAt:2,path:profile+'\\installations\\'+other,available:true}]};
  expect(parseInstallationContext(managed).installations).toEqual(managed.installations);
  const wrong={...managed,installations:[managed.installations[0],{...managed.installations[1],path:String.raw`\\?\C:\foreign`+'\\installations\\'+other}]};
  expect(()=>parseInstallationContext(wrong)).toThrow('MANAGER_PROTOCOL');
 });
 it('reports protocol failure with a safe dedicated code while hiding raw exceptions',()=>{
  expect(managerError(new Error('MANAGER_PROTOCOL')).code).toBe('MANAGER_PROTOCOL');
  expect(managerError(new Error('MANAGER_PROTOCOL SECRET=private')).code).toBe('MANAGER_CONNECTION');
 });
});
