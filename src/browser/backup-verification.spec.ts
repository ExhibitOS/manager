// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC is presentation/dispatch evidence only. Real crypto/engine is tested separately by the CLI.
for(const scenario of ['success','failure','malformed','preinstall'] as const)test(`backup ${scenario} presentation preserves authentication scope and operation exclusions`,async({page},testInfo)=>{
 await page.addInitScript(({scenario})=>{
  type FixtureWindow=Window&{__verificationCalls:number;__verificationInputs:unknown[];__resolveVerification:()=>void};
  const fixture=window as unknown as FixtureWindow;
  fixture.__verificationCalls=0;fixture.__verificationInputs=[];
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input:unknown})=>{
   if(cmd==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(cmd==='manager_external_restore')return {sources:[],checks:[]};
   if(cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_backup_tool')return scenario==='preinstall'?'sha256:'+'a'.repeat(64):null;
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:scenario!=='preinstall',bundleId:scenario==='preinstall'?null:'synthetic',version:scenario==='preinstall'?null:'0.1.0',state:scenario==='preinstall'?'not_installed':'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
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
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();const section=page.getByRole('region',{name:'백업 사본 검증'}),submit=section.getByRole('button',{name:'백업 사본 검증',exact:true});
 // Existing manual-path cases explicitly open the optional fields, preserving their validation coverage.
 for(const summary of await section.getByText('경로 직접 입력',{exact:true}).all())await summary.click();
 if(scenario!=='preinstall')await section.getByText('복구 도구 고급 설정',{exact:true}).click();
 else{await expect(section).toContainText('전시를 설치하지 않고');await expect(section.getByText('복구 도구 고급 설정',{exact:true})).toHaveCount(0);}
 await expect(section.getByLabel('암호화된 백업 폴더의 전체 경로')).toBeEnabled();
 await section.getByLabel('암호화된 백업 폴더의 전체 경로').fill('/private/tmp/synthetic-archive');
 await section.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('key contents');
 if(scenario!=='preinstall')await section.getByLabel('유지보수 이미지의 고정 ID').fill('sha256:'+'a'.repeat(64));
 await expect(submit).toBeDisabled();expect(await page.evaluate(()=>(window as unknown as {__verificationCalls:number}).__verificationCalls)).toBe(0);
 await section.getByLabel('별도로 보관한 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');
 await expect(submit).toBeEnabled();await submit.click();
 await expect(section.getByRole('progressbar',{name:'백업 인증 검사 진행 중'})).toBeVisible();
 for(const name of ['시작','재시작','실패한 작업 다시 시도'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await expect(section.getByLabel('별도로 보관한 키 파일의 전체 경로')).toBeDisabled();
 await section.locator('form').dispatchEvent('submit');expect(await page.evaluate(()=>(window as unknown as {__verificationCalls:number}).__verificationCalls)).toBe(1);
 expect(await page.evaluate(()=>(window as unknown as {__verificationInputs:unknown[]}).__verificationInputs[0])).toEqual({image:'sha256:'+'a'.repeat(64),sourcePath:'/private/tmp/synthetic-archive',keyPath:'/private/tmp/synthetic-key'});
 await page.evaluate(()=>(window as unknown as {__resolveVerification:()=>void}).__resolveVerification());
 if(scenario==='success'||scenario==='preinstall'){
  await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toBeVisible();await expect(section).toContainText('68개 파일');await expect(section).toContainText('데이터 복원은 실행하지 않았습니다.');
  await section.getByText('검사 식별 정보',{exact:true}).click();await expect(section.getByText('b'.repeat(64),{exact:true})).toBeVisible();
 }else{
  await expect(section.getByRole('alert')).toBeFocused();await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toHaveCount(0);await expect(section.getByRole('alert')).toContainText('기존 사본과 키는 보존');await expect(submit).toBeEnabled();
 }
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect(await section.locator('input').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44))).toBe(true);
  await page.screenshot({path:test.info().outputPath(`exhibitos-manager-backup-ui-${testInfo.project.name}-${scenario}-${width}.png`),fullPage:true});
 }
 if(scenario==='success'||scenario==='preinstall'){
  await section.getByLabel('암호화된 백업 폴더의 전체 경로').fill('/private/tmp/another-synthetic-archive');await expect(section.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toHaveCount(0);
 }
 expect(faults).toEqual([]);
});

test('external authenticated sources reopen independently, cancel safely and retry as fresh isolated checks',async({page})=>{
 await page.addInitScript(()=>{
  const verificationId='c30e227b-e771-43dc-b315-023e0469d4d2',backupId='e30e227b-e771-43dc-b315-023e0469d4d2',image='sha256:'+'a'.repeat(64);
  const source={verificationId,backupId,image,files:2,at:1};
  const old={id:'d30e227b-e771-43dc-b315-023e0469d4d2',verificationId,image,expectedBackupId:backupId,expectedManifestSha256:'b'.repeat(64),expectedFiles:2,jobBudgetBytes:128*1024**2,state:'completed',stage:'completed',errorCode:null,receipt:{operation:'restored-and-verified',backupId,authenticatedManifestSha256:'b'.repeat(64),files:2},createdAt:1,updatedAt:2};
  let checks=[old],calls=0,reconciles=0,finish:()=>void=()=>{},maintenance:unknown=null;
  Object.assign(window,{isTauri:true,__externalRequests:[],__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input?:Record<string,unknown>})=>{
   if(cmd==='manager_external_restore'){
    const input=args?.input;if(input?.action==='context')return {sources:[source],checks};
    (window as unknown as {__externalRequests:unknown[]}).__externalRequests.push(input);
    if(input?.action==='reconcile'){reconciles++;return {sources:[source],checks};}
    if(input?.action==='check'){
     calls++;const job={...old,id:calls===1?'f30e227b-e771-43dc-b315-023e0469d4d2':'a30e227b-e771-43dc-b315-023e0469d4d2',state:'running',stage:'checking-archive-and-restoring',receipt:null as typeof old.receipt|null,createdAt:3,updatedAt:3};checks=[...checks,job] as typeof checks;
     if(calls===1){maintenance={id:job.id,kind:'external-restoration',state:'running',stage:job.stage,errorCode:null,createdAt:3,updatedAt:3};await new Promise<void>(resolve=>{finish=()=>{Object.assign(job,{state:'interrupted',stage:'interrupted',errorCode:'CANCELLED',updatedAt:4});resolve();};});}
     else{if(reconciles!==1)throw Error('unreconciled fixture');Object.assign(job,{state:'completed',stage:'completed',receipt:old.receipt,updatedAt:5});}
     return {sources:[source],checks};
    }
   }
   if(cmd==='manager_maintenance_context')return maintenance;
   if(cmd==='manager_cancel_maintenance'){const running=maintenance as Record<string,unknown>;maintenance={...running,state:'confirmed',errorCode:'CANCELLED',updatedAt:4};finish();return {...running,state:'requested',errorCode:null,updatedAt:4};}
   if(cmd==='manager_choose_backup_path')return null;
   if(cmd==='manager_backup_tool')return image;
   if(cmd==='manager_backup_jobs'||cmd==='manager_maintenance_retries'||cmd==='manager_helper_reconciliations'||cmd==='manager_jobs'||cmd==='manager_logs')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:false,bundleId:null,version:null,state:'not_installed',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   throw Error('Unexpected synthetic command '+cmd);
  }}});
 });
 const faults:string[]=[];page.on('pageerror',e=>faults.push(e.message));
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();const section=page.getByRole('region',{name:'백업 사본 검증'});
 const history=section.getByRole('list',{name:'격리 DB 검사 기록'});await expect(history).toContainText('격리 DB 검사 완료');await expect(history).toContainText('전체 서비스·전시 공개 복원 완료가 아닙니다.');
 const select=section.getByLabel('인증한 백업 사본');await select.selectOption('c30e227b-e771-43dc-b315-023e0469d4d2');
 const key=section.getByLabel('별도로 보관한 키 파일의 전체 경로');await section.getByText('경로 직접 입력',{exact:true}).last().click();await key.fill('/private/tmp/synthetic-key');
 const check=section.getByRole('button',{name:'새 격리 DB 검사',exact:true});await expect(check).toBeEnabled();
 await section.getByRole('button',{name:'백업 키 파일 선택',exact:true}).click();await expect(key).toHaveValue('/private/tmp/synthetic-key');
 await check.click();await expect(check).toBeDisabled();
 const cancel=page.getByRole('region',{name:'진행 중 백업·복원 취소'});await expect(cancel).toContainText('격리 DB 복원 검사');await cancel.getByRole('checkbox').check();await cancel.getByRole('button',{name:'작업 취소 요청'}).click();
 await expect(history).toContainText('격리 DB 검사 중단');await expect(section).toContainText('중단한 SQL부터 이어가지 않습니다.');
 await expect(section.getByRole('button',{name:'남은 검사 도구 상태 확인'})).toBeEnabled();await section.getByRole('button',{name:'남은 검사 도구 상태 확인'}).click();await expect(section).toContainText('남은 검사 도구의 상태를 확인했습니다.');
 await expect(check).toBeEnabled();await check.click();await expect(history.getByText('격리 DB 검사 완료',{exact:true})).toHaveCount(2);
 expect(await page.evaluate(()=>(window as unknown as {__externalRequests:unknown[]}).__externalRequests)).toEqual([{action:'check',verificationId:'c30e227b-e771-43dc-b315-023e0469d4d2',keyPath:'/private/tmp/synthetic-key'},{action:'reconcile',jobId:'f30e227b-e771-43dc-b315-023e0469d4d2',preserveCandidates:true},{action:'check',verificationId:'c30e227b-e771-43dc-b315-023e0469d4d2',keyPath:'/private/tmp/synthetic-key'}]);
 await select.selectOption('');await select.selectOption('c30e227b-e771-43dc-b315-023e0469d4d2');await expect(key).toHaveValue('');await expect(check).toBeDisabled();
 await page.reload();await page.getByRole('link',{name:'백업·복원',exact:true}).click();await expect(page.getByRole('list',{name:'격리 DB 검사 기록'})).toContainText('격리 DB 검사 완료');await expect(page.getByLabel('인증한 백업 사본')).toHaveValue('');expect(faults).toEqual([]);
});
