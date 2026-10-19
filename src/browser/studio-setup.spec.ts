// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC qualifies user flow only; no native installation or server is run.
for(const scenario of ['new','ready','failure'] as const)test(`Studio preparation ${scenario} uses one explicit action and retains details`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const state={installed:scenario==='ready',bundleId:null,version:null,state:scenario==='ready'?'running':'stopped',services:[],readiness:{ready:scenario==='ready',version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
  const selection={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-studio/local-runtime',available:true}],errorCode:null};
  Object.assign(window,{isTauri:true,__studioWrites:[] as string[],__cloudReads:0,__finishSetup:null,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   const harness=window as unknown as {__studioWrites:string[];__finishSetup:()=>void};
   if(cmd==='manager_hosted_resume'||cmd==='manager_hosted_connected'){(window as unknown as {__cloudReads:number}).__cloudReads++;return cmd==='manager_hosted_resume'?null:false;}
   if(cmd==='manager_backup_tool')return null;
   if(cmd==='manager_update_cancel')return null;
   if(cmd==='manager_update_status')return {installationId:'56789012-1234-1234-1234-123456789012',authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_installations')return selection;
   if(cmd==='manager_status')return state;
   if(cmd==='manager_initial_account_context')return {state:'existing',tenantId:'11111111-1111-4111-8111-111111111111',subject:'synthetic-owner'};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_install'||cmd==='manager_action'){
    harness.__studioWrites.push(cmd);
    if(cmd==='manager_install')await new Promise<void>(resolve=>{harness.__finishSetup=resolve;});
    if(scenario==='failure')throw {code:'ENGINE_PERMISSION',guidance:'실행 도구 접근 권한을 확인하세요.'};
    if(cmd==='manager_install')state.installed=true;else{state.readiness.ready=true;state.state='running';}
    return {id:'synthetic-studio',action:cmd==='manager_install'?'install':'start',state:'completed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:null,guidance:null};
   }
   if(cmd==='manager_open_oex'){harness.__studioWrites.push(cmd);return null;}
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');const panel=page.getByRole('region',{name:'전시 만들기·이어서 편집하기'});
 const action=panel.getByRole('button',{name:scenario==='ready'?'Studio 열기':'Studio 준비하고 열기',exact:true});
 await expect(action).toBeEnabled();expect(await page.evaluate(()=>(window as unknown as {__cloudReads:number}).__cloudReads)).toBe(0);await expect(page.locator('main')).toHaveAttribute('data-manager-mode','simple');await expect(page.getByRole('region',{name:'설치된 전시 백업'})).toHaveCount(0);expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual([]);
 if(scenario==='ready'){await page.getByRole('link',{name:'백업·복원',exact:true}).click();await page.getByRole('region',{name:'백업 사본 검증'}).getByText('경로 직접 입력',{exact:true}).last().click();await page.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('/private/tmp/user-selected-key');await page.getByRole('link',{name:'전시',exact:true}).click();
  await page.getByRole('button',{name:'고급 설정',exact:true}).click();await expect.poll(()=>page.evaluate(()=>(window as unknown as {__cloudReads:number}).__cloudReads)).toBeGreaterThan(0);const firstCloudReads=await page.evaluate(()=>(window as unknown as {__cloudReads:number}).__cloudReads);await expect(page.getByRole('region',{name:'설치된 전시 백업'})).toBeVisible();await expect(page.locator('#backup-key')).toHaveValue('/private/tmp/user-selected-key');
  await page.getByRole('button',{name:'간편 화면',exact:true}).click();await expect(page.getByRole('region',{name:'설치된 전시 백업'})).toHaveCount(0);
  await page.getByRole('link',{name:'백업·복원',exact:true}).click();await expect(page.locator('#backup-key')).toHaveValue('/private/tmp/user-selected-key');await page.getByRole('link',{name:'전시',exact:true}).click();await expect(page.locator('#exhibition')).toBeFocused();
  await page.getByRole('link',{name:'배포 준비',exact:true}).click();await expect(page.getByRole('region',{name:'전시를 웹에 공개하기'})).toBeVisible();await page.getByRole('link',{name:'전시',exact:true}).click();expect(await page.evaluate(()=>(window as unknown as {__cloudReads:number}).__cloudReads)).toBe(firstCloudReads);
  await page.evaluate(()=>window.scrollTo(0,0));await page.screenshot({path:test.info().outputPath('manager-simple-studio.png'),fullPage:true});}

 await panel.getByRole('button',{name:'세부 안내 보기'}).click();await panel.getByRole('button',{name:'세부 안내 접기'}).click();if(scenario==='ready')await expect(page.locator('#backup-key')).toHaveValue('/private/tmp/user-selected-key');
 await action.click();
 if(scenario!=='ready'){
  await expect(panel.getByRole('progressbar')).toBeVisible();await expect(panel).toContainText('전시를 설치하고 있어요.');await expect(panel.getByRole('button',{name:'Studio 준비 중…'})).toBeDisabled();
  expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(['manager_install']);
  await page.evaluate(()=>(window as unknown as {__finishSetup:()=>void}).__finishSetup());
 }
 if(scenario==='failure'){await expect(page.getByRole('alert').filter({hasText:'실행 도구 접근 권한을 확인하세요.'})).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(['manager_install']);}
 else{if(scenario==='new')expect(await page.evaluate(()=>(window as unknown as {__cloudReads:number}).__cloudReads)).toBe(0);await expect(page.getByRole('status').filter({hasText:'브라우저에 Studio를 열도록 요청했어요.'})).toBeVisible();expect(await page.evaluate(()=>(window as unknown as {__studioWrites:string[]}).__studioWrites)).toEqual(scenario==='ready'?['manager_open_oex']:['manager_install','manager_action','manager_open_oex']);}
 for(const width of [320,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});

// Hold actual IPC reads instead of faking a loading flag. Background reads must
// not steal the primary action or focus; user-started setup still owns busy.
test('Studio action stays stable across delayed background refresh and invalidates unsafe state',async({page})=>{
 await page.addInitScript(()=>{
  let holdNext=true,statusCalls=0,installed=false,ready=false,mode='normal';
  const writes:string[]=[];
  let selection={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-studio/local-runtime',available:true}],errorCode:null as string|null};
  Object.assign(window,{isTauri:true,__studioRefresh:{writes,get calls(){return statusCalls;},hold(){holdNext=true;},mode(value:string){mode=value;},switchNativeSpace(){installed=false;ready=false;selection={...selection,selectionToken:'78901234-1234-1234-1234-123456789012',activeId:'67890123-1234-1234-1234-123456789012',installations:[...selection.installations,{id:'67890123-1234-1234-1234-123456789012',kind:'recovery',createdAt:2,path:'/private/tmp/synthetic-studio/installations/67890123-1234-1234-1234-123456789012',available:true}]};},release:()=>{},finish:()=>{}},__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   const control=(window as unknown as {__studioRefresh:{release:()=>void;finish:()=>void}}).__studioRefresh;
   if(cmd==='manager_installations')return mode==='selection-error'?{...selection,errorCode:'INSTALLATION_UNAVAILABLE',installations:selection.installations.map(root=>({...root,available:false}))}:selection;
   if(cmd==='manager_status'){
    const observed={installed,bundleId:null,version:null,state:ready?'running':installed?'stopped':'not_installed',services:[],readiness:{ready,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
    statusCalls++;if(holdNext){holdNext=false;await new Promise<void>(resolve=>{control.release=resolve;});}
    if(mode==='status-error')throw {code:'STATE_INVALID',guidance:'현재 전시 상태를 확인할 수 없습니다.'};
    return observed;
   }
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:mode!=='engine-unavailable',engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(cmd==='manager_initial_account_context')return {state:'existing',tenantId:'11111111-1111-4111-8111-111111111111',subject:'synthetic-owner'};
   if(cmd==='manager_install'||cmd==='manager_action'){
    writes.push(cmd);if(cmd==='manager_install'){await new Promise<void>(resolve=>{control.finish=resolve;});installed=true;}else ready=true;
    return {id:'synthetic-refresh',action:cmd==='manager_install'?'install':'start',state:'completed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:null,guidance:null};
   }
   if(cmd==='manager_open_oex'){writes.push(cmd);return null;}
   if(cmd==='manager_backup_tool'||cmd==='manager_update_cancel'||cmd==='manager_maintenance_context'||cmd==='manager_hosted_resume')return null;
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_update_status')return {installationId:selection.activeId,authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   throw Error('Unexpected synthetic command: '+cmd);
  }}});
 });
 await page.goto('/');const panel=page.getByRole('region',{name:'전시 만들기·이어서 편집하기'}),action=panel.getByRole('button',{name:'Studio 준비하고 열기',exact:true});
 await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{calls:number}}).__studioRefresh.calls)).toBe(1);await expect(action).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{release:()=>void}}).__studioRefresh.release());await expect(action).toBeEnabled();
 for(let cycle=2;cycle<=3;cycle++){
  await action.focus();await page.evaluate(()=>(window as unknown as {__studioRefresh:{hold:()=>void}}).__studioRefresh.hold());
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{calls:number}}).__studioRefresh.calls),{timeout:6000}).toBe(cycle);
  // Several observations while the same background request is still pending.
  for(let sample=0;sample<3;sample++){await expect(action).toBeEnabled();await expect(action).toBeFocused();await page.waitForTimeout(100);}
  if(cycle===3){
   await action.click();await expect(panel.getByRole('button',{name:'Studio 준비 중…'})).toBeDisabled();
   await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install']);
   await panel.getByRole('button',{name:'Studio 준비 중…'}).evaluate((button:HTMLButtonElement)=>button.click());
   expect(await page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install']);
   await page.evaluate(()=>(window as unknown as {__studioRefresh:{finish:()=>void}}).__studioRefresh.finish());
   await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install','manager_action','manager_open_oex']);
  }
  await page.evaluate(()=>(window as unknown as {__studioRefresh:{release:()=>void}}).__studioRefresh.release());
 }
 await expect(panel.getByRole('button',{name:'Studio 열기',exact:true})).toBeEnabled();
 // A completed failed observation must revoke cached readiness, unlike a read
 // which is merely pending. Reuse the existing explicit refresh action.
 await page.getByRole('button',{name:'고급 설정',exact:true}).click();
 const refresh=page.getByRole('button',{name:'실행 도구 다시 확인',exact:true});await expect(refresh).toBeEnabled();
 for(const mode of ['status-error','selection-error']){
  await page.evaluate(mode=>(window as unknown as {__studioRefresh:{mode:(value:string)=>void}}).__studioRefresh.mode(mode),mode);await refresh.click();
  await expect(panel.getByRole('button',{name:'Studio 준비하고 열기',exact:true})).toBeDisabled();
  expect(await page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install','manager_action','manager_open_oex']);
  await page.evaluate(()=>(window as unknown as {__studioRefresh:{mode:(value:string)=>void}}).__studioRefresh.mode('normal'));await expect(refresh).toBeEnabled();await refresh.click();await expect(panel.getByRole('button',{name:'Studio 열기',exact:true})).toBeEnabled();
 }
 // Engine failure must never dispatch a new installation/start, even when the
 // entry action is retained to guide the user to engine preparation.
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{mode:(value:string)=>void}}).__studioRefresh.mode('engine-unavailable'));await expect(refresh).toBeEnabled();await refresh.click();await expect(refresh).toBeEnabled();
 const open=panel.getByRole('button',{name:'Studio 열기',exact:true});if(await open.isEnabled())await open.click();
 expect(await page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install','manager_action','manager_open_oex']);
 // Synthetic external/native selection change while ready A is outstanding.
 // The UI selector correctly stays disabled during refresh; do not bypass it.
 // The fresh B response is held separately; A must not authorize B's action.
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{mode:(value:string)=>void}}).__studioRefresh.mode('normal'));await expect(refresh).toBeEnabled();await refresh.click();await expect(panel.getByRole('button',{name:'Studio 열기',exact:true})).toBeEnabled();
 const before=await page.evaluate(()=>(window as unknown as {__studioRefresh:{calls:number}}).__studioRefresh.calls);
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{hold:()=>void}}).__studioRefresh.hold());await refresh.click();
 await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{calls:number}}).__studioRefresh.calls)).toBe(before+1);
 await page.evaluate(()=>{const control=(window as unknown as {__studioRefresh:{release:()=>void;hold:()=>void}}).__studioRefresh;Object.assign(window,{__oldStudioStatusRelease:control.release});control.hold();});
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{switchNativeSpace:()=>void}}).__studioRefresh.switchNativeSpace());
 await page.evaluate(()=>(window as unknown as {__oldStudioStatusRelease:()=>void}).__oldStudioStatusRelease());
 await expect.poll(()=>page.evaluate(()=>(window as unknown as {__studioRefresh:{calls:number}}).__studioRefresh.calls),{timeout:6000}).toBe(before+2);
 await expect(panel.getByRole('button',{name:'Studio 준비하고 열기',exact:true})).toBeDisabled();
 expect(await page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install','manager_action','manager_open_oex']);
 await page.evaluate(()=>(window as unknown as {__studioRefresh:{release:()=>void}}).__studioRefresh.release());await expect(panel.getByRole('button',{name:'Studio 준비하고 열기',exact:true})).toBeEnabled();
 expect(await page.evaluate(()=>(window as unknown as {__studioRefresh:{writes:string[]}}).__studioRefresh.writes)).toEqual(['manager_install','manager_action','manager_open_oex']);
});
