// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Presentation/IPC binding only. Real filesystem preservation is covered by Rust tests.
test('synthetic space promotion preserves old space, fences requests and clears private form inputs',async({page})=>{
 await page.addInitScript(()=>{
  const old='56789012-1234-1234-1234-123456789012',fresh='67890123-1234-1234-1234-123456789012';
  let context={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:old,mode:'managed',installations:[{id:old,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
  const calls:{cmd:string;args:Record<string,unknown>}[]=[];
  Object.assign(window,{isTauri:true,__selectionCalls:calls,__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   calls.push({cmd,args});if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return structuredClone(context);
   if(args.selectionToken!==context.selectionToken)throw {code:'INSTALLATION_SELECTION_CHANGED',guidance:'관리 공간이 바뀌었습니다.'};
   if(cmd==='manager_create_installation'){context={...context,selectionToken:'78901234-1234-1234-1234-123456789012',activeId:fresh,installations:[...context.installations,{id:fresh,kind:'recovery',createdAt:2,path:`/private/tmp/synthetic-profile/installations/${fresh}`,available:true}]};return structuredClone(context);}
   if(cmd==='manager_select_installation'){context={...context,selectionToken:'89012345-1234-1234-1234-123456789012',activeId:old};return structuredClone(context);}
   if(cmd==='manager_status')return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_restoration_context')return {fresh:true,job:null,receipt:null};
   if(['manager_jobs','manager_logs','manager_backup_jobs'].includes(cmd))return [];
   throw Error('Unexpected synthetic command');
  }}});
 });
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();const region=page.getByRole('region',{name:'관리 공간 선택'}),ack=region.getByRole('checkbox');
 const create=region.getByRole('button',{name:'새 복원 공간 만들고 선택'});await expect(create).toBeDisabled();await expect(ack).toBeEnabled();
 const key=page.locator('#restoration-key');
 // Manual paths remain optional; open the actual disclosure before editing.
 await key.locator('..').getByText('경로 직접 입력',{exact:true}).click();await key.fill('/private/synthetic-external-key');await ack.check();await create.click();
 await expect(region.getByText('현재: 복원 공간',{exact:false})).toBeVisible();await expect(key).toHaveValue('');await expect(ack).not.toBeChecked();
 await region.getByRole('combobox').selectOption('56789012-1234-1234-1234-123456789012');await ack.check();await region.getByRole('button',{name:'선택한 공간 사용'}).click();await expect(region.getByText('현재: 기존 관리 공간',{exact:false})).toBeVisible();
 const calls=await page.evaluate(()=>(window as unknown as {__selectionCalls:{cmd:string;args:Record<string,unknown>}[]}).__selectionCalls);
 expect(calls.filter(c=>c.cmd==='manager_create_installation')).toHaveLength(1);expect(calls.filter(c=>c.cmd==='manager_select_installation')).toHaveLength(1);
 expect(calls.filter(c=>['manager_action','manager_restore_backup','manager_install'].includes(c.cmd))).toHaveLength(0);
 expect(calls.find(c=>c.cmd==='manager_create_installation')?.args).toEqual({selectionToken:'45678901-1234-1234-1234-123456789012',input:{preserveExisting:true}});
 expect(calls.find(c=>c.cmd==='manager_select_installation')?.args.selectionToken).toBe('78901234-1234-1234-1234-123456789012');
});
for(const mode of ['override','platform-unverified','managed-missing'] as const)test(`synthetic ${mode} preserves the recovery boundary`,async({page})=>{
 await page.addInitScript(({mode})=>{
  const id='56789012-1234-1234-1234-123456789012';
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_hosted_resume')return null;
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:id,mode:mode==='managed-missing'?'managed':mode,installations:[{id,kind:mode==='override'?'override':'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:mode!=='managed-missing'}],errorCode:mode==='managed-missing'?'INSTALLATION_ROOT_UNAVAILABLE':null};
   if(cmd==='manager_detect')return [];
   if(cmd==='manager_status'||cmd==='manager_restoration_context')throw {code:'INSTALLATION_ROOT_UNAVAILABLE',guidance:'현재 폴더 확인 필요'};
   return [];
  }}});
 },{mode});
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();const region=page.getByRole('region',{name:'관리 공간 선택'}),ack=region.getByRole('checkbox'),create=region.getByRole('button',{name:'새 복원 공간 만들고 선택'});
 if(mode==='managed-missing'){await expect(ack).toBeEnabled();await ack.check();await expect(create).toBeEnabled();await expect(page.getByRole('button',{name:'전시 설치',exact:true})).toBeDisabled();}
 else{await expect(ack).toBeDisabled();await expect(create).toBeDisabled();}
});

// Read-only polling must not interrupt an established selection or its acknowledgement.
test('held background poll retains selection controls while initial, changed and writer snapshots stay fenced',async({page})=>{
 await page.addInitScript(()=>{
  const old='56789012-1234-1234-1234-123456789012',other='67890123-1234-1234-1234-123456789012';
  let context={selectionToken:'45678901-1234-1234-1234-123456789012',activeId:old,mode:'managed',installations:[{id:old,kind:'default',createdAt:1,path:'/private/tmp/synthetic-poll/local-runtime',available:true},{id:other,kind:'recovery',createdAt:2,path:'/private/tmp/synthetic-poll/installations/'+other,available:true}],errorCode:null};
  let holdNext=true,held=false,release:()=>void=()=>{},writerHeld=false,finishWriter:()=>void=()=>{},failList=false,listFailed=false;
  const writes:string[]=[];
  Object.assign(window,{isTauri:true,__selectionPoll:{get held(){return held;},get writerHeld(){return writerHeld;},get listFailed(){return listFailed;},release(){held=false;release();},hold(){holdNext=true;},change(){context={...context,selectionToken:'78901234-1234-1234-1234-123456789012',activeId:other};holdNext=true;},finishWriter(){finishWriter();},failList(){failList=true;},writes},__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_installations'){if(failList){listFailed=true;throw {code:'STATE_UNAVAILABLE',guidance:'Synthetic selection list unavailable'};}return structuredClone(context);}
   if(cmd==='manager_select_installation'){
    if(args.selectionToken!==context.selectionToken)throw {code:'INSTALLATION_SELECTION_CHANGED',guidance:'Synthetic selection changed'};
    writes.push(cmd);writerHeld=true;await new Promise<void>(resolve=>{finishWriter=resolve;});writerHeld=false;
    context={...context,selectionToken:'89012345-1234-1234-1234-123456789012',activeId:old};return structuredClone(context);
   }
   if(cmd==='manager_status'){
    if(holdNext){holdNext=false;held=true;await new Promise<void>(resolve=>{release=resolve;});}
    return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   }
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:true,job:null,receipt:null};
   if(['manager_maintenance_context','manager_backup_tool','manager_hosted_resume','manager_update_cancel'].includes(cmd))return null;
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_external_restore'&&(args.input as {action?:string}|undefined)?.action==='context')return {sources:[],checks:[]};
   if(cmd==='manager_update_status')return {installationId:context.activeId,authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   throw Error('Unexpected synthetic selection-poll command: '+cmd);
  }}});
 });
 await page.goto('/#backup');const region=page.getByRole('region',{name:'관리 공간 선택'}),choice=region.getByRole('combobox'),ack=region.getByRole('checkbox'),use=region.getByRole('button',{name:'선택한 공간 사용'}),create=region.getByRole('button',{name:'새 복원 공간 만들고 선택'});
 const harness=()=>page.evaluate(()=>(window as unknown as {__selectionPoll:{held:boolean;writerHeld:boolean;listFailed:boolean}}).__selectionPoll.held);
 await expect.poll(harness).toBe(true);await expect(ack).toBeDisabled();await expect(create).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{release:()=>void}}).__selectionPoll.release());await expect(ack).toBeEnabled();
 await choice.selectOption('67890123-1234-1234-1234-123456789012');await ack.check();await expect(use).toBeEnabled();await expect(create).toBeEnabled();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{hold:()=>void}}).__selectionPoll.hold());await expect.poll(harness,{timeout:6000}).toBe(true);
 await expect(use).toBeEnabled({timeout:1000});await expect(create).toBeEnabled();await expect(choice).toHaveValue('67890123-1234-1234-1234-123456789012');await expect(ack).toBeChecked();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{release:()=>void}}).__selectionPoll.release());await expect(ack).toBeEnabled();
 await page.getByRole('button',{name:'고급 설정',exact:true}).click();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{hold:()=>void}}).__selectionPoll.hold());await page.getByRole('button',{name:'실행 도구 다시 확인',exact:true}).click();await expect.poll(harness).toBe(true);await expect(ack).toBeDisabled();await expect(create).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{release:()=>void}}).__selectionPoll.release());await expect(ack).toBeEnabled();await expect(ack).toBeChecked();await expect(choice).toHaveValue('67890123-1234-1234-1234-123456789012');
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{change:()=>void}}).__selectionPoll.change());await expect.poll(harness,{timeout:6000}).toBe(true);
 await expect(ack).toBeDisabled();await expect(create).toBeDisabled();await expect(ack).not.toBeChecked();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{release:()=>void}}).__selectionPoll.release());await expect(ack).toBeEnabled();
 await choice.selectOption('56789012-1234-1234-1234-123456789012');await ack.check();await use.click();
 await expect.poll(()=>page.evaluate(()=>(window as unknown as {__selectionPoll:{writerHeld:boolean}}).__selectionPoll.writerHeld)).toBe(true);await expect(use).toBeDisabled();await expect(create).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{finishWriter:()=>void}}).__selectionPoll.finishWriter());await expect(ack).toBeEnabled();await expect(ack).not.toBeChecked();await expect(choice).toHaveValue('56789012-1234-1234-1234-123456789012');
 await page.evaluate(()=>(window as unknown as {__selectionPoll:{failList:()=>void}}).__selectionPoll.failList());await expect.poll(()=>page.evaluate(()=>(window as unknown as {__selectionPoll:{listFailed:boolean}}).__selectionPoll.listFailed),{timeout:6000}).toBe(true);await expect(ack).toBeDisabled();await expect(create).toBeDisabled();
 expect(await page.evaluate(()=>(window as unknown as {__selectionPoll:{writes:string[]}}).__selectionPoll.writes)).toEqual(['manager_select_installation']);
});
