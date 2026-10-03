// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
for(const scenario of ['backup','restoration','cancel-restoration','ownership-failure','wrong-child','blocked-child','history-failure'] as const)test(`synthetic maintenance retry ${scenario} preserves the old job and binds a new job`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const kind=['restoration','cancel-restoration'].includes(scenario)?'restoration':'backup',targetId='12345678-1234-1234-1234-123456789012',id='23456789-1234-1234-1234-123456789012',child='34567890-1234-1234-1234-123456789012',source='56789012-1234-1234-1234-123456789012',destination='67890123-1234-1234-1234-123456789012';
  const original={id:targetId,operation:'create',state:'interrupted',stage:'saving-images',errorCode:'INTERRUPTED',createdAt:1,updatedAt:2};
  const restoredJob={id:targetId,state:'interrupted',stage:'authenticating',errorCode:'INTERRUPTED',createdAt:1,updatedAt:2};
  const audit={id,targetId,kind,state:'failed',newJobId:child,originalJobSha256:'a'.repeat(64),destinationRootSha256:kind==='backup'?'ac5d97aee7fbfabe891d373247a8aefdc9cbc804f2ec9ba13005f440bdb5215e':'7d277be2fe45eebc11b1551791b949f6babd62451b74f14dbb506a89320f9d12',errorCode:'CANCELLED',createdAt:1,updatedAt:2};
  const history:unknown[]=scenario==='blocked-child'?[audit]:[],inputs:unknown[]=[],cancelInputs:unknown[]=[];let resolve:()=>void=()=>{},running=false,cancelled=false;
  Object.assign(window,{isTauri:true,__retryInputs:inputs,__retryCancelInputs:cancelInputs,__resolveRetry:()=>resolve(),__originalRetryJob:original,__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:source,mode:'managed',installations:[{id:source,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true},{id:destination,kind:'recovery',createdAt:2,path:`/private/tmp/synthetic-profile/installations/${destination}`,available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:kind==='backup',bundleId:null,version:null,state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_backup_jobs')return kind==='backup'?scenario==='blocked-child'?[original,{...original,id:child,state:'failed',errorCode:'CANCELLED'}]:[original]:[];
   if(cmd==='manager_restoration_context')return {fresh:false,job:kind==='restoration'?restoredJob:null,receipt:null};
   if(cmd==='manager_maintenance_retries'){if(scenario==='history-failure')throw {code:'STATE_INVALID',guidance:'재시도 기록을 확인하세요.'};return history;}
   if(cmd==='manager_maintenance_context')return running?{id:child,kind,state:cancelled?'requested':'running',stage:'authenticating',errorCode:null,createdAt:1,updatedAt:2}:null;
   if(cmd==='manager_cancel_maintenance'){cancelInputs.push(args);cancelled=true;return {id:child,kind,state:'requested',stage:'authenticating',errorCode:null,createdAt:1,updatedAt:2};}
   if(cmd==='manager_retry_backup'||cmd==='manager_retry_restoration'){
    inputs.push({cmd,...args});running=true;await new Promise<void>(r=>{resolve=r;});running=false;
    if(scenario==='cancel-restoration'){history.push(audit);throw {code:'CANCELLED',guidance:'새 복원 작업을 취소했고 후보를 보존했습니다.'};}
    if(scenario==='ownership-failure'){history.push({...audit,newJobId:null,errorCode:'OWNERSHIP_CONFLICT'});throw {code:'OWNERSHIP_CONFLICT',guidance:'소유권이 확인되지 않아 재시도하지 않았습니다.'};}
    history.push({...audit,state:'completed',errorCode:null});
    const image=(args.input as {image:string}).image;
    const result=kind==='backup'?{id:scenario==='wrong-child'?targetId:child,backupId:id,operation:'created-and-authenticated',files:11,image,authenticatedManifestSha256:'a'.repeat(64),writersPaused:true,at:2}:{id:child,backupId:id,operation:'restored-and-running',bundleId:id,projectName:`exhibitos-${id}`,openUrl:'http://127.0.0.1:4512',authenticatedManifestSha256:'a'.repeat(64),at:2};
    return {id,targetId,newJobId:child,kind,dataPreserved:true,result};
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));await page.goto('/');const region=page.getByRole('region',{name:'중단된 백업·복원 재시도'}),submit=region.getByRole('button',{name:'새 작업으로 재시도'});
 await expect(region).toBeVisible();if(scenario==='history-failure'){await expect(submit).toBeDisabled();await expect(region).toContainText('기록을 확인할 수 없습니다');expect(await page.evaluate(()=>(window as unknown as {__retryInputs:unknown[]}).__retryInputs)).toHaveLength(0);return;}const kind=['restoration','cancel-restoration'].includes(scenario)?'restoration':'backup';await region.getByLabel('재시도할 현재 공간의 작업').selectOption(`${kind}:12345678-1234-1234-1234-123456789012`);
 if(scenario==='blocked-child'){
  await expect(submit).toBeDisabled();await expect(region).toContainText('확인이 필요한 재시도 기록');await expect(region.getByLabel('재시도할 현재 공간의 작업')).toBeEnabled();await region.getByLabel('재시도할 현재 공간의 작업').selectOption('backup:34567890-1234-1234-1234-123456789012');await region.getByLabel('별도 비공개 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');await region.getByLabel('유지보수 이미지의 고정 ID').fill(`sha256:${'a'.repeat(64)}`);await region.getByRole('checkbox').nth(0).check();await region.getByRole('checkbox').nth(1).check();await expect(submit).toBeEnabled();expect(await page.evaluate(()=>(window as unknown as {__retryInputs:unknown[]}).__retryInputs)).toHaveLength(0);return;
 }
 await region.getByLabel('별도 비공개 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');await region.getByLabel('유지보수 이미지의 고정 ID').fill(`sha256:${'a'.repeat(64)}`);
 if(kind==='restoration'){
  expect(await region.getByLabel('별도로 준비한 복원 공간').locator('option').count()).toBe(2);
  await region.getByLabel('별도로 준비한 복원 공간').selectOption('67890123-1234-1234-1234-123456789012');await region.getByLabel('복원할 암호화 사본 폴더의 전체 경로').fill('/private/tmp/synthetic-archive');await region.getByLabel('새 전시의 사용하지 않는 로컬 포트').fill('4512');
 }
 await expect(submit).toBeDisabled();await region.getByRole('checkbox').nth(0).check();await expect(submit).toBeDisabled();await region.getByRole('checkbox').nth(1).check();await expect(submit).toBeEnabled();
 await region.getByLabel('별도 비공개 키 파일의 전체 경로').fill('/private/tmp/other-key');await expect(region.getByRole('checkbox').nth(0)).not.toBeChecked();await expect(submit).toBeDisabled();await region.getByRole('checkbox').nth(0).check();await region.getByRole('checkbox').nth(1).check();await submit.click();
 await expect(region.getByRole('progressbar',{name:'유지보수 재시도 진행 중'})).toBeVisible();await region.locator('form').dispatchEvent('submit');await expect(page.getByRole('button',{name:'새 복원 공간 만들고 선택'})).toBeDisabled();await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();
 const inputs=await page.evaluate(()=>(window as unknown as {__retryInputs:{cmd:string;selectionToken:string;input:Record<string,unknown>}[]}).__retryInputs);expect(inputs).toHaveLength(1);expect(inputs[0]?.cmd).toBe(kind==='backup'?'manager_retry_backup':'manager_retry_restoration');expect(inputs[0]?.selectionToken).toBe('45678901-1234-1234-1234-123456789012');expect(inputs[0]?.input.targetId).toBe('12345678-1234-1234-1234-123456789012');expect(inputs[0]?.input.preserveCandidates).toBe(true);expect(inputs[0]?.input.keyPath).toBe('/private/tmp/other-key');if(kind==='restoration')expect(inputs[0]?.input.destinationId).toBe('67890123-1234-1234-1234-123456789012');
 if(scenario==='cancel-restoration'){const cancel=page.getByRole('region',{name:'진행 중 백업·복원 취소'});await expect(cancel).toContainText('새 설치 복원 진행 중');await cancel.getByRole('checkbox').check();await cancel.getByRole('button',{name:'작업 취소 요청'}).click();await expect(cancel).toContainText('취소를 요청했습니다');expect(await page.evaluate(()=>(window as unknown as {__retryCancelInputs:unknown[]}).__retryCancelInputs)).toEqual([{selectionToken:'45678901-1234-1234-1234-123456789012',input:{kind:'restoration',targetId:'34567890-1234-1234-1234-123456789012',preserveCandidates:true}}]);}
 await page.evaluate(()=>(window as unknown as {__resolveRetry:()=>void}).__resolveRetry());
 await expect(region.getByRole('checkbox').nth(0)).not.toBeChecked();await expect(submit).toBeDisabled();
 if(scenario==='backup'||scenario==='restoration'){await expect(region.getByRole('heading',{name:kind==='backup'?'새 백업 생성·인증 확인':'별도 공간의 새 전시 복원·시작 확인'})).toBeVisible();await expect(region).toContainText('원래 실패·중단 작업은 성공으로 변경하지 않았습니다.');if(kind==='restoration')await expect(region).toContainText('원래 공간 선택은 유지했습니다.');}
 else{await expect(region.getByRole('alert')).toBeFocused();await expect(region.getByRole('heading',{name:'새 백업 생성·인증 확인'})).toHaveCount(0);}
 await expect(region).toContainText(`목적지 공간: ${kind==='backup'?'56789012-1234-1234-1234-123456789012':'67890123-1234-1234-1234-123456789012'}`);expect(await page.evaluate(()=>(window as unknown as {__originalRetryJob:{state:string}}).__originalRetryJob.state)).toBe('interrupted');expect(errors).toEqual([]);
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await region.screenshot({path:`/private/tmp/exhibitos-manager-retry-${scenario}-${width}.png`});}
});
