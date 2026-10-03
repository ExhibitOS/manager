// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
for(const scenario of ['confirmed','uncertain','malformed','stale-poll','unknown-context'] as const)test(`active ${scenario} distinguishes request from stop and binds selection`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const id='23456789-1234-1234-1234-123456789012',inputs:unknown[]=[];let value:Record<string,unknown>={id,kind:'restoration',state:'running',stage:'authenticating',errorCode:null,createdAt:1,updatedAt:2};
  let polls=0;
  Object.assign(window,{isTauri:true,__stalePending:false,__breakContext:false,__failContext:()=>{Object.assign(window,{__breakContext:true});},__cancelInputs:inputs,__finishCancel:()=>{value={...value,state:scenario==='confirmed'||scenario==='stale-poll'?'confirmed':'uncertain',errorCode:scenario==='confirmed'||scenario==='stale-poll'?'CANCELLED':'CANCEL_UNCERTAIN',updatedAt:3};},__TAURI_INTERNALS__:{invoke:async(cmd:string,args:unknown)=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_maintenance_context'){if((window as unknown as {__breakContext:boolean}).__breakContext)throw {code:'STATE_INVALID',guidance:'현재 상태를 확인할 수 없습니다.'};polls++;if(scenario==='unknown-context')return {...value,state:'unknown'};if(scenario==='stale-poll'&&polls===2){const old={...value};return new Promise(resolve=>{Object.assign(window,{__stalePending:true,__releaseStale:()=>resolve(old)});});}return value;}
   if(cmd==='manager_cancel_maintenance'){inputs.push(args);value={...value,state:'requested'};return scenario==='malformed'?{...value,state:'confirmed',errorCode:'CANCELLED'}:value;}
   if(cmd==='manager_status')return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:{id,state:'running',stage:'authenticating',errorCode:null,createdAt:1,updatedAt:2},receipt:null};
   throw Error('Unexpected mutation');
  }}});
 },{scenario});
 await page.goto('/');const region=page.getByRole('region',{name:'진행 중 백업·복원 취소'}),button=region.getByRole('button',{name:'작업 취소 요청'});
 if(scenario==='unknown-context'){await expect(region.getByRole('alert')).toBeVisible();await expect(page.getByRole('button',{name:'새 복원 공간 만들고 선택'})).toBeDisabled();await expect(button).toHaveCount(0);return;}
 if(scenario==='stale-poll')await expect.poll(()=>page.evaluate(()=>(window as unknown as {__stalePending:boolean}).__stalePending)).toBe(true);
 await expect(button).toBeDisabled();await region.getByRole('checkbox').check();expect(await region.getByRole('checkbox').evaluate(el=>el.closest('label')!.getBoundingClientRect().height)).toBeGreaterThanOrEqual(44);await button.click();await expect(button).toBeDisabled();await expect(region.getByRole('checkbox')).not.toBeChecked();
 expect(await page.evaluate(()=>(window as unknown as {__cancelInputs:unknown[]}).__cancelInputs)).toEqual([{selectionToken:'45678901-1234-1234-1234-123456789012',input:{kind:'restoration',targetId:'23456789-1234-1234-1234-123456789012',preserveCandidates:true}}]);
 await expect(page.getByRole('button',{name:'새 복원 공간 만들고 선택'})).toBeDisabled();
 if(scenario==='stale-poll'){await page.evaluate(()=>(window as unknown as {__releaseStale:()=>void}).__releaseStale());await page.waitForTimeout(1100);await expect(button).toBeDisabled();await expect(region).toContainText('취소를 요청했습니다.');}
 if(scenario==='malformed'){await expect(region.getByRole('alert')).toBeVisible();await expect(region).not.toContainText('취소를 확인했습니다.');}
 else {await expect(region).toContainText('취소를 요청했습니다.');await expect(region).not.toContainText('취소를 확인했습니다.');await page.evaluate(()=>(window as unknown as {__finishCancel:()=>void}).__finishCancel());await expect(region).toContainText(scenario==='confirmed'||scenario==='stale-poll'?'취소를 확인했습니다.':'정지 확인이 불확실합니다.');}
 if(scenario==='confirmed'){await page.evaluate(()=>(window as unknown as {__failContext:()=>void}).__failContext());await expect(region.getByRole('alert')).toBeVisible();await expect(region).toContainText('현재 작업과 정지 여부를 확인할 수 없습니다.');await expect(region).not.toContainText('취소를 확인했습니다.');}
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});
