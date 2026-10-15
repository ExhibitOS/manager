// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC verifies consent/dispatch/uncertainty only, never real signed update qualification.
for(const scenario of ['pending','malformed-reference','schema-change'] as const)test(`registered update ${scenario} keeps dispatch explicit and results truthful`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const owner='56789012-1234-1234-1234-123456789012',referenceId='12345678-1234-1234-1234-123456789012';
  type Fixture=Window&{__updateInputs:unknown[];__finishUpdate:()=>void};const fixture=window as unknown as Fixture;fixture.__updateInputs=[];
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string,args?:{input?:unknown})=>{
   if(command==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:owner,mode:'managed',installations:[{id:owner,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(command==='manager_status')return {installed:true,bundleId:'synthetic',version:'1.0.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(command==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_maintenance_retries','manager_helper_reconciliations'].includes(command))return [];
   if(command==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(command==='manager_maintenance_context'||command==='manager_update_cancel')return null;
   if(command==='manager_update_status')return {installationId:owner,authorityPresent:true,generation:2,stage:'prepared',operationId:'update-1',failure:null,changedSchema:false,writeUncertain:false};
   if(command==='manager_update_references')return [{referenceId,operationId:'update-1',generation:2,version:'1.2.3',sourceSchema:'a'.repeat(64),targetSchema:(scenario==='schema-change'?'b':'a').repeat(64),changedSchema:scenario==='schema-change',priorQualificationRecorded:true,executionRequalificationRequired:scenario!=='malformed-reference'}];
   if(command==='manager_update_apply'){
    if(args?.input===null)return null;
    fixture.__updateInputs.push(args?.input);await new Promise<void>(resolve=>{fixture.__finishUpdate=resolve;});
    return {id:referenceId,installationId:owner,state:'awaiting-app-close',code:null,createdAt:1,requestSha256:'a'.repeat(64)};
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const panel=page.getByRole('region',{name:'업데이트 상태'});
 if(scenario==='malformed-reference'){
  await expect(panel.getByRole('alert')).toContainText('MANAGER_PROTOCOL');
  await expect(panel.getByRole('button',{name:'앱을 닫고 업데이트 적용'})).toHaveCount(0);
 }else if(scenario==='schema-change'){
  await expect(panel).toContainText('이 참조는 적용할 수 없습니다.');
  await panel.getByRole('checkbox').nth(0).check();await panel.getByRole('checkbox').nth(1).check();
  await expect(panel.getByRole('button',{name:'앱을 닫고 업데이트 적용'})).toBeDisabled();
 }else{
  const apply=panel.getByRole('button',{name:'앱을 닫고 업데이트 적용'});
  await expect(apply).toBeDisabled();
  await panel.getByRole('checkbox',{name:'외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.'}).check();await expect(apply).toBeDisabled();
  await panel.getByRole('checkbox',{name:'기존 데이터·백업·키를 보존하고 앱을 닫아 업데이트를 적용합니다. 적용 중 전시가 중단됩니다.'}).check();
  await apply.click();await expect(apply).toBeDisabled();
  await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__updateInputs:unknown[]}).__updateInputs)).toEqual([{referenceId:'12345678-1234-1234-1234-123456789012',operationId:'update-1',generation:2,preserveData:true,closeAppAndApply:true,externalWritersQuiesced:true}]);
  await page.evaluate(()=>(window as unknown as {__finishUpdate:()=>void}).__finishUpdate());
  await expect(panel).toContainText('앱 종료 또는 실행 대기');await expect(panel).not.toContainText('업데이트 적용 완료 기록');
  await expect(apply).toBeDisabled();
 }
 expect(await page.evaluate(()=>(window as unknown as {__updateInputs:unknown[]}).__updateInputs.length)).toBe(scenario==='pending'?1:0);
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});
