// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
for(const scenario of ['override','platform-unverified','device-refused'] as const)test(`Windows canonical profile ${scenario} keeps IPC boundary and readable recovery`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const calls:string[]=[];
  const slash=String.fromCharCode(92);
  const path=scenario==='device-refused'?slash+slash+'.'+slash+'PhysicalDrive0':slash+slash+'?'+slash+'C:'+slash+'Users'+slash+'Jung'+slash+'AppData'+slash+'Local'+slash+'ExhibitOS-GuiCheck-synthetic';
  Object.assign(window,{isTauri:true,__windowsGuiCalls:calls,__TAURI_INTERNALS__:{invoke:async(command:string)=>{
   calls.push(command);
   if(command==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:scenario==='platform-unverified'?'platform-unverified':'override',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:scenario==='platform-unverified'?'default':'override',createdAt:1,path,available:true}],errorCode:null};
   if(command==='manager_status')return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(command==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(command==='manager_restoration_context')return {fresh:true,job:null,receipt:null};
   if(command==='manager_maintenance_context')return null;
   if(command==='manager_jobs'||command==='manager_logs'||command==='manager_backup_jobs'||command==='manager_helper_reconciliations'||command==='manager_maintenance_retries')return [];
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');
 const controls=page.getByRole('region',{name:'전시 설치와 실행'});
 if(scenario==='device-refused'){
  await expect(page.getByText('확인 코드: MANAGER_PROTOCOL',{exact:false}).first()).toBeVisible();
  await expect(controls.getByRole('button',{name:'전시 설치',exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as unknown as {__windowsGuiCalls:string[]}).__windowsGuiCalls.every(c=>c==='manager_installations'))).toBe(true);
 }else{
  await expect(controls).toContainText('Docker 사용 가능');
  await expect(page.getByRole('region',{name:'실제 전시 상태'})).toContainText('설치 전');
  await expect(page.getByText('확인 코드: MANAGER_CONNECTION',{exact:false})).toHaveCount(0);
  const before=await page.evaluate(()=>(window as unknown as {__windowsGuiCalls:string[]}).__windowsGuiCalls.filter(c=>c==='manager_detect').length);
  await controls.getByRole('button',{name:'실행 도구 다시 확인'}).click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__windowsGuiCalls:string[]}).__windowsGuiCalls.filter(c=>c==='manager_detect').length)).toBeGreaterThan(before);
  await expect(controls).toContainText('Docker 사용 가능');
 }
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
});
