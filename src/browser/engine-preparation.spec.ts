// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
test.beforeEach(({browser})=>{test.info().annotations.push({type:'actual-browser',description:`${browser.browserType().name()} ${browser.version()}`});});
// Closed synthetic IPC. No network download, official installer or engine operation executes here.
for(const scenario of ['progress','permission','hash','delayed-prepare','delayed-cancel'] as const)test(`official engine preparation ${scenario} preserves boundaries`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  let ready=false,holdStatus=scenario==='delayed-prepare';const writes:string[]=[];let releaseStatus:(()=>void)|null=null;
  let prepared={jobId:'11111111-1111-4111-8111-111111111111',stage:'not-started',savedBytes:0,totalBytes:589939097,version:'4.95.0',errorCode:null as string|null};
  const saved=sessionStorage.getItem('synthetic-engine-cache');if(saved)prepared=JSON.parse(saved);
  Object.assign(window,{isTauri:true,__engineWrites:writes,__engineStatusHeld:false,__holdNextEngineStatus:()=>{holdStatus=true;},__releaseEngineStatus:()=>{releaseStatus?.();releaseStatus=null;},__advanceEngineProgress:()=>{prepared.savedBytes=200;},__engineReady:()=>{ready=true;},__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>)=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-engine/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:ready,available:ready,engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context'||cmd==='manager_update_cancel')return null;
   if(cmd==='manager_update_status')return {installationId:'56789012-1234-1234-1234-123456789012',authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_engine_preparation'){
    const action=args.action as string;if(action==='status'&&holdStatus){holdStatus=false;const snapshot=structuredClone(prepared);Object.assign(window,{__engineStatusHeld:true});return new Promise(resolve=>{releaseStatus=()=>{Object.assign(window,{__engineStatusHeld:false});resolve(snapshot);};});}if(action!=='status'){writes.push(action);if(action==='prepare'){prepared.savedBytes=100;prepared.stage=scenario==='permission'?'awaiting-installation':scenario==='hash'?'failed':'downloading';prepared.errorCode=scenario==='permission'?'ENGINE_PREP_OS_PERMISSION':scenario==='hash'?'ENGINE_PREP_HASH':null;}if(action==='cancel')prepared.stage='cancelled';sessionStorage.setItem('synthetic-engine-cache',JSON.stringify(prepared));}return structuredClone(prepared);
   }
   if(cmd==='manager_install'||cmd==='manager_action')throw Error('Engine preparation must not install/start Platform automatically');
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const panel=page.getByRole('region',{name:'실행 도구 준비',exact:true});await expect(panel).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__engineWrites:string[]}).__engineWrites)).toEqual([]);
 await panel.getByRole('button',{name:'공식 실행 도구 준비',exact:true}).click();
 if(scenario==='delayed-prepare'||scenario==='delayed-cancel'){
  const progress=panel.getByRole('progressbar',{name:'공식 설치 파일 다운로드'});
  await expect(progress).toHaveAttribute('value','100');
  if(scenario==='delayed-cancel')await page.evaluate(()=>(window as unknown as {__holdNextEngineStatus:()=>void}).__holdNextEngineStatus());
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__engineStatusHeld:boolean}).__engineStatusHeld)).toBe(true);
  if(scenario==='delayed-cancel'){
   await panel.getByRole('button',{name:'준비 중지 요청'}).click();
   await expect(panel.getByRole('status').filter({hasText:'준비를 멈췄어요.'})).toBeVisible();
  }
  await page.evaluate(async()=>{(window as unknown as {__releaseEngineStatus:()=>void}).__releaseEngineStatus();await new Promise<void>(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve())));});
  if(scenario==='delayed-prepare'){
   await expect(progress).toHaveAttribute('value','100');
   await expect(panel.getByRole('button',{name:'준비 중지 요청'})).toBeEnabled();
   await expect(page.getByRole('button',{name:'Studio 준비하고 열기',exact:true})).toBeDisabled();
  }else{
   await expect(panel.getByRole('status').filter({hasText:'준비를 멈췄어요.'})).toBeVisible();
   await expect(panel.getByRole('button',{name:'준비 중지 요청'})).toHaveCount(0);
   await panel.getByRole('button',{name:'공식 실행 도구 준비',exact:true}).click();
   await expect(progress).toHaveAttribute('value','100');
  }
  await page.evaluate(()=>(window as unknown as {__advanceEngineProgress:()=>void}).__advanceEngineProgress());
  await expect(progress).toHaveAttribute('value','200');
  expect(await page.evaluate(()=>(window as unknown as {__engineWrites:string[]}).__engineWrites)).toEqual(scenario==='delayed-prepare'?['prepare']:['prepare','cancel','prepare']);
 }else if(scenario==='progress'){
  await expect(panel.getByRole('progressbar',{name:'공식 설치 파일 다운로드'})).toHaveAttribute('value','100');await panel.getByRole('button',{name:'준비 중지 요청'}).click();await expect(panel.getByRole('status').filter({hasText:'준비를 멈췄어요.'})).toBeVisible();
  await page.reload();await expect(panel.getByRole('status').filter({hasText:'준비를 멈췄어요.'})).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__engineWrites:string[]}).__engineWrites)).toEqual([]);await panel.getByRole('button',{name:'공식 실행 도구 준비',exact:true}).click();await expect(panel.getByRole('progressbar')).toHaveAttribute('value','100');
 }else if(scenario==='permission'){await expect(panel.getByRole('status')).toContainText('macOS 승인');await expect(panel.getByRole('button',{name:'공식 설치 창 다시 열기'})).toBeVisible();await expect(panel).not.toContainText('실행 도구를 사용할 수 있어요.');}
 else{await expect(panel.getByRole('alert')).toContainText('설치 파일을 확인하지 못해 실행하지 않았어요.');await expect(panel.getByRole('button',{name:'공식 설치 창 다시 열기'})).toHaveCount(0);}
 for(const width of [320,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});
