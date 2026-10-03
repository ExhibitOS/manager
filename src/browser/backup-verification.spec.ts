// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC is presentation/dispatch evidence only. Real crypto/engine is tested separately by the CLI.
for(const scenario of ['success','failure','malformed'] as const)test(`backup ${scenario} presentation preserves authentication scope and operation exclusions`,async({page},testInfo)=>{
 await page.addInitScript(({scenario})=>{
  type FixtureWindow=Window&{__verificationCalls:number;__verificationInputs:unknown[];__resolveVerification:()=>void};
  const fixture=window as unknown as FixtureWindow;
  fixture.__verificationCalls=0;fixture.__verificationInputs=[];
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input:unknown})=>{
   if(cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs')return [];
   if(cmd==='manager_verify_backup'){
    fixture.__verificationCalls++;fixture.__verificationInputs.push(args?.input);
    await new Promise<void>(resolve=>{fixture.__resolveVerification=resolve;});
    if(scenario==='failure')throw {code:'BACKUP_VERIFICATION_FAILED',guidance:'기존 사본과 키를 보존하고 새 검증 작업으로 확인하세요.'};
    return {id:'12345678-1234-1234-1234-123456789012',operation:scenario==='malformed'?'restored':'verified',files:68,image:'sha256:'+'a'.repeat(64),authenticatedManifestSha256:'b'.repeat(64),at:2};
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 const faults:string[]=[];page.on('pageerror',error=>faults.push(error.message));
 await page.goto('/');const section=page.getByRole('region',{name:'백업 사본 검증'}),submit=section.getByRole('button',{name:'백업 사본 검증',exact:true});
 await expect(section.getByLabel('암호화된 백업 폴더의 전체 경로')).toBeEnabled();
 await section.getByLabel('암호화된 백업 폴더의 전체 경로').fill('/private/tmp/synthetic-archive');
 await section.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('key contents');
 await section.getByLabel('유지보수 이미지의 고정 ID').fill('sha256:'+'a'.repeat(64));
 await expect(submit).toBeDisabled();expect(await page.evaluate(()=>(window as unknown as {__verificationCalls:number}).__verificationCalls)).toBe(0);
 await section.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');
 await expect(submit).toBeEnabled();await submit.click();
 await expect(section.getByRole('progressbar',{name:'백업 인증 검사 진행 중'})).toBeVisible();
 for(const name of ['시작','재시작','실패한 작업 다시 시도'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await expect(section.getByLabel('별도로 보관한 키 파일의 전체 경로')).toBeDisabled();
 await section.locator('form').dispatchEvent('submit');expect(await page.evaluate(()=>(window as unknown as {__verificationCalls:number}).__verificationCalls)).toBe(1);
 expect(await page.evaluate(()=>(window as unknown as {__verificationInputs:unknown[]}).__verificationInputs[0])).toEqual({image:'sha256:'+'a'.repeat(64),sourcePath:'/private/tmp/synthetic-archive',keyPath:'/private/tmp/synthetic-key'});
 await page.evaluate(()=>(window as unknown as {__resolveVerification:()=>void}).__resolveVerification());
 if(scenario==='success'){
  await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toBeVisible();await expect(section).toContainText('68개 파일');await expect(section).toContainText('데이터 복원은 실행하지 않았습니다.');
  await section.getByText('검사 식별 정보',{exact:true}).click();await expect(section.getByText('b'.repeat(64),{exact:true})).toBeVisible();
 }else{
  await expect(section.getByRole('alert')).toBeFocused();await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toHaveCount(0);await expect(section.getByRole('alert')).toContainText('기존 사본과 키는 보존');await expect(submit).toBeEnabled();
 }
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect(await section.locator('input').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44))).toBe(true);
  await page.screenshot({path:`/private/tmp/exhibitos-manager-backup-ui-${testInfo.project.name}-${scenario}-${width}.png`,fullPage:true});
 }
 if(scenario==='success'){
  await section.getByLabel('암호화된 백업 폴더의 전체 경로').fill('/private/tmp/another-synthetic-archive');await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toHaveCount(0);
 }
 expect(faults).toEqual([]);
});
