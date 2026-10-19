// SPDX-License-Identifier: Apache-2.0
import {test,expect,type Page} from '@playwright/test';
// Synthetic IPC/real React components only. No engine, native helper, backup or signed qualification runs here.
const id='12345678-1234-4234-8234-123456789012',installationId='56789012-1234-4234-8234-123456789012',token='45678901-1234-4234-8234-123456789012';
async function fixture(page:Page,mode:'blocked'|'error'|'form'|'delayed'|'reopen'){
 page.on('pageerror',error=>console.error('Synthetic preparation page: '+error.message));
 await page.addInitScript(({mode,id,installationId})=>{
  const f=window as unknown as {calls:{command:string;args:unknown}[];releaseInspection:()=>void;unblock:boolean;inputs:unknown[]};f.calls=[];f.inputs=[];f.unblock=false;
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string,args?:{input?:{id?:string;selectionToken?:string}})=>{
   f.calls.push({command,args});
   const job={id,installationId,state:'running',phase:'original-backup',code:null,referenceId:null,updatedAt:1,canCancel:false,canRetry:false};
   if(command==='manager_update_preparation_startup'){
    if(mode==='error'&&!f.unblock)throw Error('Synthetic unavailable');
    return {controllerBlocked:mode==='blocked'&&!f.unblock,canOpenManager:mode==='reopen',job:mode==='reopen'?{...job,state:'prepared',phase:'registered',referenceId:id}:mode==='blocked'&&!f.unblock?job:null};
   }
   if(command==='manager_update_preparation_status'){f.unblock=true;return {controllerBlocked:false,canOpenManager:false,job:{...job,state:'prepared',phase:'registered',referenceId:id}};}
   if(command==='manager_update_preparation_open_manager')return null;
   if(command==='manager_update_preparation_choose_release')return '/private/tmp/synthetic-release';
   if(command==='manager_update_preparation_inspect'){
    if(mode==='delayed')await new Promise<void>(resolve=>{f.releaseInspection=resolve;});
    return {available:true,code:null,version:'1.2.3',changedSchema:false};
   }
   if(command==='manager_update_preparation_dispatch'){f.inputs.push(args?.input);return {...job,state:'awaiting-app-close',phase:'validating-inputs',canCancel:true};}
   if(command==='manager_update_preparation_cancel')return {controllerBlocked:false,canOpenManager:false,job:{...job,state:'cancelled-before-start',phase:'validating-inputs'}};
   throw Error('Unexpected synthetic command');
  }}});
 },{mode,id,installationId});
 // The harness supplies only parent props. Production gate/component/IPC parsers remain unchanged.
 await page.route('**/preparation-fixture',route=>route.fulfill({contentType:'text/html',body:`<html lang="ko"><meta name="viewport" content="width=device-width,initial-scale=1"><div id="root"></div><script type="module">
 import {injectIntoGlobalHook} from '/@react-refresh';injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;
 const {mountPreparationFixture}=await import('/src/browser/preparation-fixture.tsx');mountPreparationFixture(document.getElementById('root'),'${token}','${installationId}');
 </script></html>`}));
 await page.goto('/preparation-fixture');
}
async function selectInputs(page:Page){await page.getByRole('button',{name:'업데이트 폴더 선택',exact:true}).click();await page.getByText('경로 직접 입력',{exact:true}).last().click();await page.getByLabel('별도로 보관한 백업 키 파일',{exact:true}).fill('/private/tmp/synthetic.key');}
test('startup blocks normal effects until fresh native startup permits them',async({page})=>{
 await fixture(page,'blocked');await expect(page.getByRole('region',{name:'업데이트 준비 상태'})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {managerMounted?:number}).managerMounted??0)).toBe(0);
 await expect(page.getByRole('button',{name:'시작 전 준비 취소'})).toHaveCount(0);
 await expect(page.locator('progress')).not.toHaveAttribute('value',/.+/);
 await page.getByRole('button',{name:'상태 다시 확인'}).click();await expect(page.getByRole('heading',{name:'Manager 테스트 부모'})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {managerMounted:number}).managerMounted)).toBe(1);
 const calls=await page.evaluate(()=>(window as unknown as {calls:{command:string}[]}).calls.map(c=>c.command));expect(calls.slice(0,3)).toEqual(['manager_update_preparation_startup','manager_update_preparation_status','manager_update_preparation_startup']);
});
test('startup failure keeps normal effects unmounted and has no protocol fallback',async({page})=>{
 await fixture(page,'error');await expect(page.getByRole('alert')).toContainText('일반 관리 작업은 열지 않았습니다');expect(await page.evaluate(()=>(window as unknown as {managerMounted?:number}).managerMounted??0)).toBe(0);
});
test('inspection invalidates on input changes; explicit consent dispatches once with dark and narrow layout',async({page},testInfo)=>{
 await page.addInitScript(()=>localStorage.setItem('exhibitos.manager.appearance','dark'));
 await fixture(page,'form');await selectInputs(page);await page.getByRole('button',{name:'준비 자료 검사',exact:true}).click();
 const dispatch=page.getByRole('button',{name:'앱을 닫고 업데이트 준비',exact:true});await expect(dispatch).toBeDisabled();
 await page.getByRole('checkbox').nth(0).check();await page.getByRole('checkbox').nth(1).check();await expect(dispatch).toBeEnabled();
 await page.getByLabel('별도로 보관한 백업 키 파일',{exact:true}).fill('/private/tmp/other.key');await expect(dispatch).toHaveCount(0);
 await page.getByRole('button',{name:'준비 자료 검사',exact:true}).click();await expect(dispatch).toBeDisabled();
 await page.getByRole('checkbox').nth(0).check();await page.getByRole('checkbox').nth(1).check();
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);if(width===320)await page.screenshot({path:testInfo.outputPath('preparation-narrow-dark.png'),fullPage:true});}
 // Theme selection is the unchanged production control, exposed while the handoff blocks Manager.
 await dispatch.evaluate(el=>{(el as HTMLButtonElement).click();(el as HTMLButtonElement).click();});await expect(page.getByRole('region',{name:'업데이트 준비 상태'})).toBeVisible();await expect(page.locator('html')).toHaveAttribute('data-appearance','dark');
 const inputs=await page.evaluate(()=>(window as unknown as {inputs:unknown[]}).inputs);expect(inputs).toEqual([{selectionToken:token,releaseFolder:'/private/tmp/synthetic-release',keyFile:'/private/tmp/other.key',preserveData:true,externalWritersQuiesced:true,closeAppAndPrepare:true}]);
 await page.screenshot({path:testInfo.outputPath('preparation-dark.png'),fullPage:true});await page.getByLabel('화면 모드',{exact:true}).selectOption('light');await expect(page.locator('html')).toHaveAttribute('data-appearance','light');await page.screenshot({path:testInfo.outputPath('preparation-light.png'),fullPage:true});
});
test('late inspection from a previous selection never enables preparation',async({page})=>{
 await fixture(page,'delayed');await selectInputs(page);await page.getByRole('button',{name:'준비 자료 검사',exact:true}).click();await expect(page.getByText('준비 자료를 확인하고 있습니다',{exact:true})).toBeVisible();
 await page.getByRole('button',{name:'테스트 관리 공간 전환'}).click();await page.evaluate(()=>(window as unknown as {releaseInspection:()=>void}).releaseInspection());
 await expect(page.getByRole('button',{name:'앱을 닫고 업데이트 준비'})).toHaveCount(0);await expect(page.getByRole('checkbox')).toHaveCount(0);await expect(page.getByText('선택한 업데이트 폴더가 없습니다.',{exact:true})).toBeVisible();
});

test('terminal waiting process restarts natively then restores updates once before normal effects',async({page})=>{
 await fixture(page,'reopen');await expect(page.getByRole('button',{name:'Manager로 돌아가기',exact:true})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {managerMounted?:number}).managerMounted??0)).toBe(0);
 await page.getByRole('button',{name:'Manager로 돌아가기',exact:true}).click();
 expect(await page.evaluate(()=>(window as unknown as {managerMounted?:number}).managerMounted??0)).toBe(0);
 expect(await page.evaluate(()=>(window as unknown as {calls:{command:string}[]}).calls.map(c=>c.command))).toEqual(['manager_update_preparation_startup','manager_update_preparation_open_manager']);
 expect(await page.evaluate(()=>localStorage.getItem('exhibitos.manager.preparation.resumeView'))).toBe('updates');
 // Simulate the next, Controller-ready process with a new real component mount and normal native startup reply.
 await fixture(page,'form');await expect(page.getByRole('heading',{name:'Manager 테스트 부모'})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {mountHash:string}).mountHash)).toBe('#updates');
 expect(await page.evaluate(()=>localStorage.getItem('exhibitos.manager.preparation.resumeView'))).toBeNull();
 await page.goto('/preparation-fixture#exhibition');await page.reload();await expect(page.getByRole('heading',{name:'Manager 테스트 부모'})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {mountHash:string}).mountHash)).toBe('#exhibition');
});
