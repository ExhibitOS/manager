// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC tests the GUI boundary only; native save panels, random keys and Docker remain separate.
test.beforeEach(async({page})=>{
 await page.addInitScript(()=>{
  const ids=['56789012-1234-1234-1234-123456789012','67890123-1234-1234-1234-123456789012'];let selected=0;
  const w=window as unknown as {__path:string|null;__defer:boolean;__finishPicker:(value:string|null)=>void;__running:boolean;__calls:{cmd:string;kind?:unknown}[]};
  Object.assign(w,{__path:'/private/tmp/synthetic-external.key',__defer:false,__running:true,__calls:[]});
  const context=()=>({selectionToken:['45678901-1234-1234-1234-123456789012','78901234-1234-1234-1234-123456789012'][selected],activeId:ids[selected],mode:'managed',installations:ids.map((id,index)=>({id,kind:index?'recovery':'default',createdAt:1,path:index?'/private/tmp/synthetic-profile/installations/'+id:'/private/tmp/synthetic-profile/local-runtime',available:true})),errorCode:null});
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_installations')return context();
   if(cmd==='manager_select_installation'){selected=ids.indexOf(args.targetId as string);return context();}
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:w.__running?'running':'stopped',services:[],readiness:{ready:w.__running,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:0,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:null,composeVersion:null,errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_maintenance_retries','manager_helper_reconciliations'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context'||cmd==='manager_update_cancel')return null;
   if(cmd==='manager_update_status')return {installationId:ids[selected],authorityPresent:false,generation:null,stage:null,operationId:null,failure:null,changedSchema:false,writeUncertain:false};
   if(cmd==='manager_backup_tool')return 'sha256:'+'a'.repeat(64);
   if(cmd==='manager_choose_backup_path'){w.__calls.push({cmd,kind:args.kind});if(w.__defer)return new Promise<string|null>(resolve=>{w.__finishPicker=resolve;});return w.__path;}
   if(cmd==='manager_hosted_connected')return false;
   if(cmd==='manager_hosted_resume')return null;
   w.__calls.push({cmd});throw Error('Unexpected synthetic mutation');
  }}});
 });
});
test('backup preparation chooses paths, preserves cancelled input and uses bundled metadata without manual SHA copying',async({page})=>{
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));await page.goto('/');await page.getByRole('link',{name:'백업·복원',exact:true}).click();
 const region=page.getByRole('region',{name:'설치된 전시 백업'});
 await expect(region).toContainText('이 Manager에 포함된 복구 도구를 선택했습니다.');await expect(region.locator('#creation-image')).toHaveValue('sha256:'+'a'.repeat(64));await expect(region.locator('#creation-image')).toBeHidden();
 await region.getByRole('button',{name:'백업 키 파일 선택',exact:true}).click();await expect(region).toContainText('선택됨: synthetic-external.key');await expect(region.locator('#creation-key')).toHaveValue('/private/tmp/synthetic-external.key');
 await page.evaluate(()=>{(window as unknown as {__path:null}).__path=null;});await region.getByRole('button',{name:'백업 키 파일 선택',exact:true}).click();await expect(region.locator('#creation-key')).toHaveValue('/private/tmp/synthetic-external.key');
 await page.evaluate(()=>{(window as unknown as {__path:string}).__path='/private/tmp/synthetic-new.key';});await region.getByRole('button',{name:'새 백업 키 만들기',exact:true}).click();await expect(region).toContainText('선택됨: synthetic-new.key');
 await region.getByText('복구 도구 고급 설정',{exact:true}).click();await region.locator('#creation-image').fill('sha256:'+'b'.repeat(64));await page.getByRole('link',{name:'전시',exact:true}).click();await page.getByRole('link',{name:'백업·복원',exact:true}).click();await expect(region.locator('#creation-image')).toHaveValue('sha256:'+'b'.repeat(64));
 for(const box of await region.getByRole('checkbox').all())await box.check();await expect(region.getByRole('button',{name:'전시를 멈추고 백업 생성',exact:true})).toBeEnabled();
 await page.evaluate(()=>{(window as unknown as {__running:boolean}).__running=false;});await expect(region).toContainText('실행 중인 데이터베이스가 필요합니다.');await expect(region.getByRole('button',{name:'전시를 멈추고 백업 생성',exact:true})).toBeDisabled();await expect(region.getByRole('button',{name:'백업 키 파일 선택',exact:true})).toBeEnabled();
 expect(await page.evaluate(()=>(window as unknown as {__calls:unknown[]}).__calls)).toEqual([{cmd:'manager_choose_backup_path',kind:'key'},{cmd:'manager_choose_backup_path',kind:'key'},{cmd:'manager_choose_backup_path',kind:'new_key'}]);expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);expect(errors).toEqual([]);
});
test('a delayed native path response cannot populate another selected installation',async({page})=>{
 await page.goto('/');await page.getByRole('link',{name:'백업·복원',exact:true}).click();const region=page.getByRole('region',{name:'설치된 전시 백업'});await page.evaluate(()=>{(window as unknown as {__defer:boolean}).__defer=true;});await region.getByRole('button',{name:'백업 키 파일 선택',exact:true}).click();await expect(region.getByRole('button',{name:'선택 창을 확인하세요…',exact:true})).toBeDisabled();await expect.poll(()=>page.evaluate(()=>typeof (window as unknown as {__finishPicker:unknown}).__finishPicker)).toBe('function');
 await page.getByRole('button',{name:'고급 설정',exact:true}).click();const select=page.getByLabel('등록된 공간');await select.selectOption('67890123-1234-1234-1234-123456789012');
 // Switching is an explicit preservation action, not a mutation of the old installation.
 await page.getByRole('checkbox',{name:'기존 데이터가 보존되고 기존 서버는 자동 정지되지 않는 것을 확인했습니다.'}).check();await page.getByRole('button',{name:'선택한 공간 사용',exact:true}).click();
 await page.evaluate(()=>(window as unknown as {__finishPicker:(value:string)=>void}).__finishPicker('/private/tmp/late-old.key'));await expect(region.locator('#creation-key')).toHaveValue('');await expect(region).not.toContainText('late-old.key');
});
