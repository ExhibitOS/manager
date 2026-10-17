// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC qualifies user flow only; no native installation or server is run.
for(const scenario of ['new','ready','failure'] as const)test(`Studio preparation ${scenario} uses one explicit action and retains details`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const state={installed:scenario==='ready',bundleId:null,version:null,state:'stopped',services:[],readiness:{ready:scenario==='ready',version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
  const selection={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-studio/local-runtime',available:true}],errorCode:null};
  Object.assign(window,{isTauri:true,__studioWrites:[] as string[],__finishSetup:null,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   const harness=window as unknown as {__studioWrites:string[];__finishSetup:()=>void};
   if(cmd==='manager_hosted_resume')return null;
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_update_cancel')return null;
   if(cmd==='manager_update_status')return {installationId:'56789012-1234-1234-1234-123456789012',authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_installations')return selection;
   if(cmd==='manager_status')return state;
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_install'||cmd==='manager_action'){
    harness.__studioWrites.push(cmd);
    if(cmd==='manager_install')await new Promise<void>(resolve=>{harness.__finishSetup=resolve;});
    if(scenario==='failure')throw {code:'ENGINE_PERMISSION',guidance:'실행 도구 접근 권한을 확인하세요.'};
    if(cmd==='manager_install')state.installed=true;else state.readiness.ready=true;
    return {id:'synthetic-studio',action:cmd==='manager_install'?'install':'start',state:'completed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:null,guidance:null};
   }
   if(cmd==='manager_open_oex'){harness.__studioWrites.push(cmd);return null;}
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const panel=page.getByRole('region',{name:'전시 만들기·이어서 편집하기'});
 const action=panel.getByRole('button',{name:scenario==='ready'?'Studio 열기':'Studio 준비하고 열기',exact:true});
 await expect(action).toBeEnabled();expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual([]);
 if(scenario==='ready')await page.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('/private/tmp/user-selected-key');
 await panel.getByRole('button',{name:'세부 안내 보기'}).click();await panel.getByRole('button',{name:'세부 안내 접기'}).click();if(scenario==='ready')await expect(page.getByLabel('별도로 보관한 키 파일의 전체 경로')).toHaveValue('/private/tmp/user-selected-key');
 await action.click();
 if(scenario!=='ready'){
  await expect(panel.getByRole('progressbar')).toBeVisible();await expect(panel).toContainText('전시를 설치하고 있어요.');await expect(panel.getByRole('button',{name:'Studio 준비 중…'})).toBeDisabled();
  expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(['manager_install']);
  await page.evaluate(()=>(window as unknown as {__finishSetup:()=>void}).__finishSetup());
 }
 if(scenario==='failure'){await expect(page.getByRole('alert').filter({hasText:'실행 도구 접근 권한을 확인하세요.'})).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(['manager_install']);}
 else{await expect(page.getByRole('status').filter({hasText:'브라우저에 Studio를 열도록 요청했어요.'})).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(scenario==='ready'?['manager_open_oex']:['manager_install','manager_action','manager_open_oex']);}
 for(const width of [320,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});
