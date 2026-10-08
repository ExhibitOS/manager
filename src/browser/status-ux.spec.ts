// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC replies qualify presentation only, never native engine success.
for(const scenario of ['running','failed','ready'] as const)test(`synthetic ${scenario} status is readable and retains action boundaries`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const job={id:'synthetic-ux',action:'start',state:scenario==='ready'?'completed':scenario,attempt:1,progress:30,createdAt:1,updatedAt:2,errorCode:scenario==='failed'?'ENGINE_PERMISSION':null,guidance:scenario==='failed'?'실행 도구 접근 권한을 확인하세요. 데이터는 보존됩니다.':null};
  const status={installed:true,bundleId:'synthetic',version:'0.1.0',state:scenario==='ready'?'running':'stopped',services:[{name:'synthetic-service-with-a-long-name-for-narrow-layout',state:'running',health:'healthy'}],readiness:{ready:scenario==='ready',version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:scenario==='running'?job:null};
  Object.assign(window,{isTauri:true,__uxCalls:[],__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   (window as unknown as {__uxCalls:string[]}).__uxCalls.push(cmd);
   if(cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return status;
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs')return [job];
   if(cmd==='manager_logs')return [];
   if(cmd==='manager_action'){job.state='completed';job.progress=100;job.guidance=null;return job;}
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const state=page.getByRole('region',{name:'실제 전시 상태'});await expect(state.getByText('실행 중 · 응답 정상')).toBeVisible();await expect(page.getByRole('heading',{name:'작업 기록'})).toBeVisible();
 if(scenario==='running'){await expect(page.getByRole('progressbar',{name:'현재 작업 진행률'})).toHaveAttribute('value','30');for(const name of ['시작','정지','재시작','전시 열기','실패한 작업 다시 시도'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();}
 if(scenario==='failed'){await expect(page.getByRole('region',{name:'작업 진행과 복구'})).toContainText('실행 도구 접근 권한을 확인하세요');await expect(page.getByRole('complementary',{name:'다음 할 일'})).toContainText('복구 안내');}
 if(scenario==='ready'){await expect(page.getByRole('button',{name:'전시 열기',exact:true})).toBeEnabled();await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();}
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect(await page.locator('button:visible').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44&&e.getBoundingClientRect().width>=24))).toBe(true);
  await page.getByRole('link',{name:'전시 관리로 건너뛰기'}).focus();await page.keyboard.press('Enter');await expect(page.getByRole('region',{name:'전시 설치와 실행'})).toBeFocused();await page.keyboard.press('Tab');expect(await page.evaluate(()=>{const e=document.activeElement as HTMLElement,s=getComputedStyle(e);return e.matches(':focus-visible')&&parseFloat(s.outlineWidth)>=2;})).toBe(true);
  await page.screenshot({path:test.info().outputPath(`exhibitos-manager-ux-${scenario}-${width}.png`),fullPage:true});
 }
 if(scenario==='failed'){await page.getByRole('button',{name:'실패한 작업 다시 시도'}).click();await expect(page.getByRole('button',{name:'실패한 작업 다시 시도'})).toBeDisabled();expect(await page.evaluate(()=>(window as unknown as {__uxCalls:string[]}).__uxCalls.filter(c=>c==='manager_action').length)).toBe(1);}
});
