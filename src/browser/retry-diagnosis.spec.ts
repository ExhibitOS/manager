// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
for(const scenario of ['no-child','child-found','unproven','contradictory','changed-receipt','pending'] as const)test(`synthetic retry diagnosis ${scenario} keeps candidates and requires explicit recovery`,async({page})=>{
 await page.addInitScript(({scenario})=>{
  const id='23456789-1234-1234-1234-123456789012',target='12345678-1234-1234-1234-123456789012',child='34567890-1234-1234-1234-123456789012',source='56789012-1234-1234-1234-123456789012',destination='67890123-1234-1234-1234-123456789012';
  const kind=scenario==='child-found'?'restoration':'backup';
  const audit={id,targetId:target,kind,state:'preparing',newJobId:null,originalJobSha256:'a'.repeat(64),destinationRootSha256:kind==='backup'?'ac5d97aee7fbfabe891d373247a8aefdc9cbc804f2ec9ba13005f440bdb5215e':'7d277be2fe45eebc11b1551791b949f6babd62451b74f14dbb506a89320f9d12',errorCode:null,createdAt:1,updatedAt:2};
  const calls:unknown[]=[];let resolve=()=>{};
  Object.assign(window,{isTauri:true,__diagnosisCalls:calls,__finishDiagnosis:()=>resolve(),__TAURI_INTERNALS__:{invoke:async(cmd:string,args:Record<string,unknown>={})=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:source,mode:'managed',installations:[{id:source,kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true},{id:destination,kind:'recovery',createdAt:2,path:`/private/tmp/synthetic-profile/installations/${destination}`,available:true}],errorCode:'STATE_INVALID'};
   if(cmd==='manager_status'||cmd==='manager_backup_jobs'||cmd==='manager_restoration_context')throw {code:'STATE_INVALID',guidance:'합성 불완전 후보'};
   if(cmd==='manager_detect'||cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [audit];
   if(cmd==='manager_diagnose_retry'){
    calls.push({cmd,...args});if(scenario==='pending')await new Promise<void>(r=>{resolve=r;});
    return {retryId:id,targetId:target,kind,outcome:scenario==='unproven'||scenario==='contradictory'?'unproven':kind==='restoration'?'child-found':'no-child-created',newJobId:child,childJobSha256:kind==='restoration'?'b'.repeat(64):null,canReconcile:scenario!=='unproven',dataPreserved:true};
   }
   if(cmd==='manager_reconcile_retry'){
    calls.push({cmd,...args});audit.state='failed';audit.errorCode='RETRY_NOT_STARTED' as never;
    return {diagnosisId:'78901234-1234-1234-1234-123456789012',retryId:id,outcome:scenario==='changed-receipt'?'child-found':kind==='restoration'?'child-found':'no-child-created',newJobId:scenario==='changed-receipt'||kind==='restoration'?child:null,dataPreserved:true};
   }
   throw Error(`unexpected synthetic command ${cmd}`);
  }}});
 },{scenario});
 await page.goto('/');const panel=page.getByRole('heading',{name:'재시도 연결 진단',exact:true}).locator('..');
 await panel.getByLabel('진단할 재시도 기록').selectOption('23456789-1234-1234-1234-123456789012');
 const diagnose=panel.getByRole('button',{name:'기록을 변경하지 않고 진단',exact:true});await expect(diagnose).toBeEnabled();
 if(scenario==='pending'){
  await diagnose.evaluate((b:HTMLButtonElement)=>{b.click();b.click();});await expect(panel.getByRole('button',{name:'기록 확인 중…'})).toBeDisabled();
  expect(await page.evaluate(()=> (window as unknown as {__diagnosisCalls:unknown[]}).__diagnosisCalls.length)).toBe(1);
  await page.evaluate(()=> (window as unknown as {__finishDiagnosis:()=>void}).__finishDiagnosis());
 }else await diagnose.click();
 const recover=panel.getByRole('button',{name:'증거를 다시 검사하고 연결 기록 복구',exact:true});
 if(scenario==='unproven'){await expect(panel.getByText('준비 증거가 없어 연결을 추측하지 않습니다。'.replace('。','.'))).toBeVisible();await expect(recover).toHaveCount(0);return;}
 if(scenario==='contradictory'){await expect(panel.getByRole('alert')).toBeVisible();await expect(recover).toHaveCount(0);return;}
 await expect(recover).toBeDisabled();await panel.getByRole('checkbox',{name:'원래 기록·후보·데이터를 보존하고 검증된 재시도 연결 기록만 복구하는 데 동의합니다.'}).check();await recover.click();
 if(scenario==='changed-receipt'){await expect(panel.getByRole('alert')).toBeVisible();await expect(panel.getByText('연결 기록 복구 완료', {exact:true})).toHaveCount(0);}else await expect(panel.getByText('연결 기록 복구 완료',{exact:true})).toBeVisible();
 const calls=await page.evaluate(()=> (window as unknown as {__diagnosisCalls:{cmd:string;input:{retryId:string;destinationId:string|null;preserveCandidates:boolean};selectionToken:string}[]}).__diagnosisCalls);
 expect(calls).toHaveLength(2);expect(calls[0]?.input).toEqual({retryId:'23456789-1234-1234-1234-123456789012',destinationId:scenario==='child-found'?'67890123-1234-1234-1234-123456789012':null,preserveCandidates:false});expect(calls[1]?.input.preserveCandidates).toBe(true);expect(calls[1]?.selectionToken).toBe('45678901-1234-1234-1234-123456789012');
 if(scenario==='no-child'||scenario==='child-found')for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await panel.screenshot({path:`/private/tmp/exhibitos-diagnosis-${scenario}-${width}-${test.info().project.name}.png`});}

});
