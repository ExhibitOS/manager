// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC exercises guarded UI dispatch only; no archive, key, engine or real rollback is used.
for(const scenario of ['restore-pending','complete-uncertain','stale-context','selection-reconciliation','both-actions'] as const)test(`full rollback ${scenario} preserves explicit intent and truthful completion`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const owner='56789012-1234-1234-1234-123456789012',id='12345678-1234-1234-1234-123456789012',complete=scenario==='complete-uncertain';
  type Fixture=Window&{__rollbackInputs:unknown[];__finishRollback:()=>void};const fixture=window as unknown as Fixture;fixture.__rollbackInputs=[];const choices={archive:0,key:0};
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string,args?:{input?:unknown;kind?:'archive'|'key'})=>{
   if(command==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:owner,mode:'managed',installations:[{id:owner,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(command==='manager_status')return {installed:true,bundleId:'synthetic',version:'1.0.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(command==='manager_backup_tool')return 'sha256:'+'e'.repeat(64);
   if(command==='manager_choose_backup_path'){if(!args?.kind)throw Error('Missing chooser kind');return ++choices[args.kind]===1?null:args.kind==='archive'?'/private/tmp/synthetic-original-backup':'/private/tmp/synthetic-external-key';}
   if(command==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_maintenance_retries','manager_helper_reconciliations'].includes(command))return [];
   if(command==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(['manager_maintenance_context','manager_update_cancel','manager_update_apply'].includes(command))return null;
   if(command==='manager_update_status')return {installationId:owner,authorityPresent:true,generation:4,stage:complete?'awaiting_rollback_health':'recovery_required',operationId:'update-1',failure:'health_failed',changedSchema:true,writeUncertain:false};
   if(command==='manager_update_rollback_context')return {installationId:owner,operationId:'update-1',generation:scenario==='stale-context'?3:4,stage:complete?'awaiting_rollback_health':'recovery_required',backupId:id,sourceImage:'a'.repeat(64),sourceSchema:'b'.repeat(64),backupManifest:'c'.repeat(64),sourceInventory:'d'.repeat(64),canRestoreOriginal:!complete,canCompleteRestored:complete||scenario==='both-actions',selectionReconciliationRequired:scenario==='selection-reconciliation'};
   if(command==='manager_update_rollback'){
    if(args?.input===null)return null;
    fixture.__rollbackInputs.push(args?.input);
    if(!complete)await new Promise<void>(resolve=>{fixture.__finishRollback=resolve;});
    return {id,installationId:owner,state:complete?'uncertain':'awaiting-app-close',code:complete?'UPDATE_WRITE_UNCERTAIN':null,createdAt:1,requestSha256:'a'.repeat(64)};
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 await page.goto('/');if(scenario!=='restore-pending')await page.getByRole('button',{name:'고급 설정',exact:true}).click();const panel=page.getByRole('region',{name:'업데이트 상태'});
 if(scenario==='restore-pending'){await expect(page.locator('main')).toHaveAttribute('data-manager-mode','simple');await expect(panel).toBeVisible();await expect(panel).toHaveAttribute('data-needs-attention','true');await page.getByRole('button',{name:'고급 설정',exact:true}).click();}
 if(scenario==='stale-context'){
  await expect(panel.getByRole('alert')).toContainText('MANAGER_PROTOCOL');
  await expect(panel.getByRole('button',{name:'앱을 닫고 원래 전시 전체 복원'})).toHaveCount(0);

 }else{
  if(scenario==='selection-reconciliation')await expect(panel).toContainText('선택 기록 확인·복구와 실제 응답 확인을 이어갑니다.');
  if(scenario==='both-actions'){
   await expect(panel.getByRole('button',{name:'앱을 닫고 복원된 전시 확인'})).toBeDisabled();
   await expect(panel.getByLabel('원래 전체 백업 폴더의 전체 경로')).toHaveCount(0);
   await expect(panel.getByText('이 Manager에 포함된 복구 도구를 선택했습니다. 실행할 때 이미지와 백업의 호환성을 다시 확인합니다.')).toBeVisible();
   await panel.getByRole('checkbox',{name:'외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.'}).check();
   await panel.getByRole('checkbox',{name:'전시 중단을 이해하며, 현재 전시·후보·백업·키를 보존하고 앱을 닫아 원래 전시 전체 복원을 진행합니다.'}).check();
   await expect(panel.getByRole('button',{name:'앱을 닫고 복원된 전시 확인'})).toBeEnabled();
   await panel.getByText('복원 방법 선택',{exact:true}).click();
   await panel.getByLabel('진행할 복원 방법').selectOption('restoreOriginal');
   await expect(panel.getByRole('checkbox',{name:'외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.'})).not.toBeChecked();
   await expect(panel.getByRole('checkbox',{name:'전시 중단을 이해하며, 현재 전시·후보·백업·키를 보존하고 앱을 닫아 원래 전시 전체 복원을 진행합니다.'})).not.toBeChecked();
   await expect(panel.getByLabel('복원 후보의 로컬 포트')).toBeVisible();
  }
  const complete=scenario==='complete-uncertain',button=panel.getByRole('button',{name:complete?'앱을 닫고 복원된 전시 확인':'앱을 닫고 원래 전시 전체 복원'});
  await expect(button).toBeDisabled();
  await expect(panel.getByText('이 Manager에 포함된 복구 도구를 선택했습니다. 실행할 때 이미지와 백업의 호환성을 다시 확인합니다.')).toBeVisible();
  if(complete){await expect(panel.getByLabel('원래 전체 백업 폴더의 전체 경로')).toHaveCount(0);await expect(panel.getByLabel('복원에 사용할 별도 키 파일의 전체 경로')).toHaveCount(0);}
  else{
   for(const name of ['백업 폴더 선택','백업 키 파일 선택']){await panel.getByRole('button',{name,exact:true}).click();await expect(button).toBeDisabled();await panel.getByRole('button',{name,exact:true}).click();}
   await expect(panel.getByText('선택됨: synthetic-original-backup',{exact:true})).toBeVisible();
   await expect(panel.getByText('선택됨: synthetic-external-key',{exact:true})).toBeVisible();
   if(scenario==='restore-pending'){
    await panel.getByText('복구 도구 고급 설정',{exact:true}).click();
    await panel.getByLabel('복원용 유지보수 이미지의 고정 ID').fill('sha256:'+'f'.repeat(64));
    await panel.getByRole('button',{name:'업데이트 상태 확인',exact:true}).click();
    await expect(panel.getByRole('button',{name:'포함된 복구 도구 사용',exact:true})).toBeVisible();
    await expect(panel.getByLabel('복원용 유지보수 이미지의 고정 ID')).toHaveValue('sha256:'+'f'.repeat(64));
    await expect(panel.getByText('선택됨: synthetic-original-backup',{exact:true})).toBeVisible();
    await expect(panel.getByText('선택됨: synthetic-external-key',{exact:true})).toBeVisible();
    await panel.getByRole('button',{name:'포함된 복구 도구 사용',exact:true}).click();
   }
   await panel.getByLabel('복원 후보의 로컬 포트').fill('1023');
  }
  await panel.getByRole('checkbox',{name:'외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.'}).check();
  await expect(button).toBeDisabled();
  await panel.getByRole('checkbox',{name:'전시 중단을 이해하며, 현재 전시·후보·백업·키를 보존하고 앱을 닫아 원래 전시 전체 복원을 진행합니다.'}).check();
  if(!complete){await expect(button).toBeDisabled();await panel.getByLabel('복원 후보의 로컬 포트').fill('4500');
   // Changing the destination clears previous consent.
   await expect(button).toBeDisabled();
   await panel.getByRole('checkbox',{name:'외부에서 이 전시에 쓰는 작업을 중지했습니다. 앱은 실제 실행 상태도 다시 확인합니다.'}).check();
   await panel.getByRole('checkbox',{name:'전시 중단을 이해하며, 현재 전시·후보·백업·키를 보존하고 앱을 닫아 원래 전시 전체 복원을 진행합니다.'}).check();}
  await button.click();
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__rollbackInputs:unknown[]}).__rollbackInputs)).toEqual([{action:complete?'completeRestored':'restoreOriginal',operationId:'update-1',generation:4,maintenanceImage:'sha256:'+'e'.repeat(64),keyFile:complete?null:'/private/tmp/synthetic-external-key',archive:complete?null:'/private/tmp/synthetic-original-backup',port:complete?null:4500,preserveData:true,closeAppAndRollback:true,externalWritersQuiesced:true}]);
  if(!complete){await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();await page.evaluate(()=>(window as unknown as {__finishRollback:()=>void}).__finishRollback());await expect(panel).toContainText('전체 복원 요청: 앱 종료 또는 실행 대기');}
  else await expect(panel).toContainText('전체 복원 요청: 완료 여부 확인 불가');
  await expect(panel).not.toContainText('원래 전시 전체 복원 완료 기록');
  await expect(button).toBeDisabled();
 }
 expect(await page.evaluate(()=>(window as unknown as {__rollbackInputs:unknown[]}).__rollbackInputs.length)).toBe(scenario==='stale-context'?0:1);
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
});
