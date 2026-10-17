// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC only. No native account or DB is created.
for(const scenario of ['required','existing'] as const)test(`first account ${scenario} preserves native credential boundary`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const selection={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-account/local-runtime',available:true}],errorCode:null};
  const state={installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
  const writes:string[]=[];
  Object.assign(window,{isTauri:true,__accountWrites:writes,__accountInputAccepted:false,__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>)=>{
   if(cmd==='manager_installations')return selection;
   if(cmd==='manager_status')return state;
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context'||cmd==='manager_update_cancel')return null;
   if(cmd==='manager_update_status')return {installationId:selection.activeId,authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_initial_account_context')return scenario==='required'?{state:'required',tenantId:null,subject:null}:{state:'existing',tenantId:'11111111-1111-4111-8111-111111111111',subject:'synthetic-owner'};
   if(cmd==='manager_install'||cmd==='manager_action'){writes.push(cmd);if(cmd==='manager_install'){
    const input=args.initialAccount as {subject:string;password:string}|null;
    if(scenario==='required'&&(!input||input.subject!=='new-owner'||input.password!=='synthetic-user-password'))throw Error('synthetic initial input missing');
    if(scenario==='existing'&&input!==null)throw Error('existing account must not change');
    (window as unknown as {__accountInputAccepted:boolean}).__accountInputAccepted=true;state.installed=true;
   }else{state.readiness.ready=true;state.state='running';}
   return {id:'synthetic-account-job',action:cmd==='manager_install'?'install':'start',state:'completed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:null,guidance:null};}
   if(cmd==='manager_open_oex'){writes.push(cmd);return null;}
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const start=page.getByRole('button',{name:'Studio 준비하고 열기',exact:true});await expect(start).toBeEnabled();await start.click();
 const form=page.getByRole('form',{name:'처음 사용할 계정 설정'});
 if(scenario==='required'){
  await expect(form).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__accountWrites:string[]}).__accountWrites)).toEqual([]);
  await form.getByRole('button',{name:'취소',exact:true}).click();await expect(form).toHaveCount(0);expect(await page.evaluate(()=>(window as unknown as {__accountWrites:string[]}).__accountWrites)).toEqual([]);
  await expect(start).toBeEnabled();await start.click();await expect(form).toBeVisible();await form.getByLabel('계정',{exact:true}).fill('new-owner');await form.getByLabel('비밀번호',{exact:true}).fill('synthetic-user-password');await form.getByLabel('비밀번호 확인',{exact:true}).fill('mismatched-password');await form.getByRole('button',{name:'계정 설정하고 계속'}).click();await expect(form.getByRole('alert')).toHaveText('두 비밀번호가 다릅니다. 다시 확인하세요.');expect(await page.evaluate(()=>(window as unknown as {__accountWrites:string[]}).__accountWrites)).toEqual([]);await form.getByLabel('비밀번호 확인',{exact:true}).fill('synthetic-user-password');await form.getByRole('button',{name:'계정 설정하고 계속'}).click();
 }
 await expect(page.getByRole('status').filter({hasText:'브라우저에 Studio를 열도록 요청했어요.'})).toBeVisible();await expect(form).toHaveCount(0);expect(await page.evaluate(()=>(window as unknown as {__accountInputAccepted:boolean}).__accountInputAccepted)).toBe(true);expect(await page.evaluate(()=>(window as unknown as {__accountWrites:string[]}).__accountWrites)).toEqual(['manager_install','manager_action','manager_open_oex']);expect(await page.locator('body').innerText()).not.toContain('synthetic-user-password');expect(await page.evaluate(()=>localStorage.length)).toBe(0);
});
