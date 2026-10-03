// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC exercises presentation and request binding; actual engine proof is separate.
for(const scenario of ['backup-stopped','restoration-absent','ownership-failure','malformed-receipt'] as const)test(`synthetic ${scenario} requires acknowledgement and preserves the failed job`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const kind=scenario==='restoration-absent'?'restoration':'backup',id='23456789-1234-1234-1234-123456789012',rid='12345678-1234-1234-1234-123456789012';
  const original={id,operation:'create',state:'failed',stage:'encrypting-and-authenticating',errorCode:'ENGINE_TIMEOUT',createdAt:1,updatedAt:2};
  const restoreJob={id,state:'interrupted',stage:'authenticating',errorCode:'INTERRUPTED',createdAt:1,updatedAt:2};
  const history:unknown[]=[],inputs:unknown[]=[];let resolve:()=>void=()=>{};
  Object.assign(window,{isTauri:true,__helperInputs:inputs,__resolveHelper:()=>resolve(),__originalHelperJob:original,__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:kind==='backup',bundleId:kind==='backup'?'synthetic':null,version:null,state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs')return [];
   if(cmd==='manager_backup_jobs')return kind==='backup'?[original]:[];
   if(cmd==='manager_restoration_context')return {fresh:false,job:kind==='restoration'?restoreJob:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_helper_reconciliations')return history;
   if(cmd==='manager_reconcile_helper'){
    inputs.push(args);await new Promise<void>(r=>{resolve=r;});
    const success=scenario==='backup-stopped'||scenario==='restoration-absent',helperState=scenario==='restoration-absent'?'absent':'stopped';
    history.push({id:rid,targetId:id,kind,state:success?'completed':'failed',helperState:success?helperState:null,errorCode:success?null:'OWNERSHIP_CONFLICT',createdAt:1,updatedAt:2});
    if(scenario==='ownership-failure')throw {code:'OWNERSHIP_CONFLICT',guidance:'소유권이 확인되지 않아 정지하지 않았습니다.'};
    return {id:rid,targetId:id,kind,operation:'helper-reconciled',helperState,dataPreserved:true,writersResumed:scenario==='malformed-receipt',at:2};
   }
   throw Error('Unexpected synthetic mutation');
  }}});
 },{scenario});
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));await page.goto('/');const region=page.getByRole('region',{name:'중단된 작업의 helper 확인'}),submit=region.getByRole('button',{name:'해당 helper 확인·정지'});
 await expect(region).toBeVisible();await expect(submit).toBeDisabled();const kind=scenario==='restoration-absent'?'restoration':'backup';await region.getByRole('combobox').selectOption(`${kind}:23456789-1234-1234-1234-123456789012`);await expect(submit).toBeDisabled();await region.getByRole('checkbox').check();await expect(submit).toBeEnabled();await submit.click();
 await expect(region.getByRole('progressbar',{name:'helper 정지 여부 확인 중'})).toBeVisible();await region.locator('form').dispatchEvent('submit');await expect(page.getByRole('button',{name:'새 복원 공간 만들고 선택'})).toBeDisabled();await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();
 const inputs=await page.evaluate(()=>(window as unknown as {__helperInputs:unknown[]}).__helperInputs);expect(inputs).toEqual([{selectionToken:'45678901-1234-1234-1234-123456789012',input:{kind,targetId:'23456789-1234-1234-1234-123456789012',preserveCandidates:true}}]);
 await page.evaluate(()=>(window as unknown as {__resolveHelper:()=>void}).__resolveHelper());await expect(region.getByRole('checkbox')).not.toBeChecked();await expect(submit).toBeDisabled();
 if(scenario==='backup-stopped'||scenario==='restoration-absent'){await expect(region.getByRole('heading',{name:scenario==='backup-stopped'?'helper 정지 확인':'helper 부재 확인'})).toBeVisible();await expect(region).toContainText('원래 백업·복원은 완료로 바꾸지 않으며');}
 else{await expect(region.getByRole('alert')).toBeFocused();await expect(region.getByRole('heading',{name:/helper (정지|부재) 확인/})).toHaveCount(0);}
 expect(await page.evaluate(()=>(window as unknown as {__originalHelperJob:{state:string}}).__originalHelperJob.state)).toBe('failed');expect(errors).toEqual([]);
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await region.screenshot({path:`/private/tmp/exhibitos-manager-helper-${scenario}-${width}.png`});}
});
