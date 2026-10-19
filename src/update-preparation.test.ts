// SPDX-License-Identifier: Apache-2.0
import {describe,it,expect} from 'vitest';
import {parsePreparationReceipt,parsePreparationStatus,parsePreparationInspection,validPreparationPath} from './update-preparation';
const id='12345678-1234-4234-8234-123456789012',installationId='56789012-1234-4234-8234-123456789012';
const pending={id,installationId,state:'awaiting-app-close',phase:'validating-inputs',code:null,referenceId:null,updatedAt:1,canCancel:true,canRetry:false};
describe('native update preparation protocol',()=>{
 it('accepts exact observed phases and keeps prepared distinct from applied',()=>{
  expect(parsePreparationStatus({controllerBlocked:true,canOpenManager:false,job:pending}).job?.canCancel).toBe(true);
  const prepared={...pending,state:'prepared',phase:'registered',referenceId:id,canCancel:false};expect(parsePreparationReceipt(prepared).state).toBe('prepared');
  for(const state of ['failed','interrupted'])expect(parsePreparationReceipt({...pending,state,phase:'full-qualification',code:'UPDATE_PREPARATION_FAILED',canCancel:false}).state).toBe(state);
  expect(parsePreparationStatus({controllerBlocked:false,canOpenManager:false,job:null}).job).toBeNull();
  expect(parsePreparationStatus({controllerBlocked:false,canOpenManager:true,job:{...prepared}}).canOpenManager).toBe(true);
 });
 it('rejects extra fields, unknown states, forged readiness and unsafe diagnostics',()=>{
  for(const invalid of [{...pending,password:'secret'},{...pending,state:'failed',canCancel:false},{...pending,state:'interrupted',canCancel:false},{...pending,id:'ABCDEF12-1234-4234-8234-123456789012'},{...pending,state:'updated'},{...pending,updatedAt:8.64e15+1},{...pending,updatedAt:1.1},{...pending,code:'/private/path'},{...pending,canRetry:true},{...pending,state:'running',canCancel:true},{...pending,state:'prepared',phase:'checkpoint',referenceId:id,canCancel:false},{...pending,state:'prepared',phase:'registered',canCancel:false},{...pending,state:'cancelled-before-start',phase:'checkpoint',canCancel:false}])expect(()=>parsePreparationReceipt(invalid)).toThrow('MANAGER_PROTOCOL');
  for(const value of [{controllerBlocked:true,canOpenManager:false,job:null},{controllerBlocked:true,canOpenManager:true,job:pending},{controllerBlocked:false,canOpenManager:false,job:pending},{controllerBlocked:false,canOpenManager:false,job:null,approved:true}])expect(()=>parsePreparationStatus(value)).toThrow('MANAGER_PROTOCOL');
 });
 it('accepts unavailable inspection without inventing readiness and validates only path syntax',()=>{
  expect(parsePreparationInspection({available:false,code:'UPDATE_PREPARATION_RUNTIME_UNAVAILABLE',version:null,changedSchema:null}).available).toBe(false);
  for(const value of [{available:true,code:null,version:'1',changedSchema:false,approved:true},{available:true,code:'unsafe message',version:null,changedSchema:null},{available:'true',code:null,version:null,changedSchema:null}])expect(()=>parsePreparationInspection(value)).toThrow('MANAGER_PROTOCOL');
  for(const path of ['/private/tmp/release','C:\\synthetic\\release','\\\\host\\share'])expect(validPreparationPath(path)).toBe(true);
  for(const path of ['', 'relative/release','/private/tmp/secret\nvalue'])expect(validPreparationPath(path)).toBe(false);
 });
});
