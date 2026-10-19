// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Native detection, OS selection and launch are mocked; native read-only/FS proof is separate.
for(const scenario of ['detected','choose','cancel','error'] as const)test(`optional processing ${scenario} stays independent of Studio`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const app={path:'/Applications/Synthetic.app',name:'Synthetic Processing',version:'1',bundleId:scenario==='detected'?'org.exhibitos.processing':'test.compatible'};
  let selected=scenario==='cancel'?app:null;const context=()=>({supported:true,detectedCount:scenario==='detected'?1:0,detected:scenario==='detected'?app:null,selected,selectionUnavailable:false,storeAvailable:false});
  Object.assign(window,{isTauri:true,__processingCalls:[],__lifecycleMutations:[],__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{action?:string})=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_processing_app'){
    (window as unknown as {__processingCalls:unknown[]}).__processingCalls.push(args);
    if(scenario==='error')throw {code:'PROCESSING_DETECTION_FAILED',guidance:'앱 목록을 확인하지 못했어요. Studio는 계속 사용할 수 있어요.'};
    if(args?.action==='choose'){await new Promise<void>(resolve=>{(window as unknown as {__resolveChoice:()=>void}).__resolveChoice=resolve;});if(scenario!=='cancel')selected=app;}
    return context();
   }
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'running',services:[],readiness:{ready:true,version:null,protocolVersion:'1',errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs'||cmd==='manager_helper_reconciliations'||cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_action'||cmd==='manager_install'){(window as unknown as {__lifecycleMutations:unknown[]}).__lifecycleMutations.push(args);throw Error('No lifecycle action requested');}
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 const faults:string[]=[];page.on('pageerror',e=>faults.push(e.message));await page.goto('/');const region=page.getByRole('region',{name:'촬영 파일을 모델로 만들기'}),studio=page.getByRole('button',{name:'Studio 열기',exact:true});
 await expect(studio).toBeEnabled();expect(await page.evaluate(()=>(window as unknown as {__processingCalls:unknown[]}).__processingCalls)).toEqual([]);
 await region.getByText('선택적 처리 앱 연결',{exact:true}).click();await expect(region).toContainText('공식 Mac App Store 제품 페이지는 준비 중');await expect(region.getByRole('link')).toHaveCount(0);
 if(scenario==='error')await expect(region.getByRole('alert')).toContainText('Studio는 계속');
 else if(scenario==='detected'){
  await expect(region).toContainText('정식 스토어 설치·서명 여부는 확인하지 않았어요.');await region.getByRole('button',{name:'ExhibitOS Processing 열기'}).click();await expect(region).toContainText('앱 열기를 요청했습니다.');
 }else{
  if(scenario==='cancel')await expect(region).toContainText('선택한 도구: Synthetic Processing');else await expect(region).toContainText('앱을 찾지 못했어요');
  const choose=region.getByRole('button',{name:'사용할 처리 앱 선택'});await choose.click();await expect(choose).toBeDisabled();await choose.dispatchEvent('click');await page.evaluate(()=>(window as unknown as {__resolveChoice:()=>void}).__resolveChoice());await expect(choose).toBeEnabled();await expect(region).toContainText('선택한 도구: Synthetic Processing');
  if(scenario==='choose'){await region.getByRole('button',{name:'선택한 처리 앱 열기'}).click();await expect(region).toContainText('앱 열기를 요청했습니다.');}
 }
 await expect(studio).toBeEnabled();expect(await page.evaluate(()=>(window as unknown as {__lifecycleMutations:unknown[]}).__lifecycleMutations)).toEqual([]);expect(await page.evaluate(()=>(window as unknown as {__processingCalls:{action:string}[]}).__processingCalls.filter(v=>v.action==='choose'))).toHaveLength(scenario==='choose'||scenario==='cancel'?1:0);expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);expect(faults).toEqual([]);
});
