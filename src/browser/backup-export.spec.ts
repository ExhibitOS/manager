// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Actual browser actions with synthetic IPC; OS chooser/native encrypted copy are separately qualified.
for(const outcome of ['success','cancel','failure'] as const)test(`completed backup export ${outcome} preserves history and shares only selected copy`,async({page})=>{
 await page.addInitScript(({outcome})=>{
  const id='12345678-1234-1234-1234-123456789012',image='sha256:'+'a'.repeat(64);
  Object.assign(window,{isTauri:true,__exportCalls:[],__verifyInputs:[],__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{jobId?:string;input?:unknown})=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_helper_reconciliations'||cmd==='manager_maintenance_retries')return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_backup_tool')return image;
   if(cmd==='manager_backup_jobs')return [{id,operation:'create',state:'completed',stage:'complete',errorCode:null,createdAt:1,updatedAt:2}];
   if(cmd==='manager_export_backup'){
    (window as unknown as {__exportCalls:unknown[]}).__exportCalls.push(args);await new Promise<void>(resolve=>{(window as unknown as {__resolveExport:()=>void}).__resolveExport=resolve;});
    if(outcome==='cancel')return null;if(outcome==='failure')throw {code:'BACKUP_EXPORT_IO',guidance:'보관할 공간을 확인하고 같은 백업의 보관만 다시 시도하세요.'};
    return {jobId:id,backupId:'23456789-1234-1234-1234-123456789012',path:'/private/tmp/external/ExhibitOS-backup-copy',files:15,bytes:1234};
   }
   if(cmd==='manager_choose_backup_path')return '/private/tmp/synthetic-external.key';
   if(cmd==='manager_verify_backup'){(window as unknown as {__verifyInputs:unknown[]}).__verifyInputs.push(args?.input);return {id:'34567890-1234-1234-1234-123456789012',operation:'verified',files:12,image,authenticatedManifestSha256:'b'.repeat(64),at:3};}
   throw Error('Unexpected synthetic command '+cmd);
  }}});
 },{outcome});
 const faults:string[]=[];page.on('pageerror',e=>faults.push(e.message));await page.goto('/#backup');
 const creation=page.getByRole('region',{name:'설치된 전시 백업'}),verify=page.getByRole('region',{name:'백업 사본 검증'}),button=creation.getByRole('button',{name:'백업 사본 보관',exact:true});
 await expect(button).toBeEnabled();await button.click();await expect(button).toBeDisabled();await page.evaluate(()=>(window as unknown as {__resolveExport:()=>void}).__resolveExport());await expect(button).toBeEnabled();
 await expect(creation).toContainText('생성 완료');expect(await page.evaluate(()=>(window as unknown as {__exportCalls:unknown[]}).__exportCalls)).toHaveLength(1);
 if(outcome==='success'){
  await expect(creation.getByRole('heading',{name:'암호화 백업 사본을 외부에 보관했습니다.'})).toBeVisible();await expect(verify).toContainText('방금 외부에 보관한 사본을 선택했습니다.');
  await verify.getByRole('button',{name:'백업 키 파일 선택',exact:true}).click();await verify.getByRole('button',{name:'백업 사본 검증',exact:true}).click();await expect(verify.getByRole('heading',{name:'백업 무결성 인증을 통과했습니다.'})).toBeVisible();
  expect(await page.evaluate(()=>(window as unknown as {__verifyInputs:unknown[]}).__verifyInputs)).toEqual([{image:'sha256:'+'a'.repeat(64),keyPath:'/private/tmp/synthetic-external.key',sourcePath:'/private/tmp/external/ExhibitOS-backup-copy'}]);
 }else{
  await expect(creation.getByRole('heading',{name:'암호화 백업 사본을 외부에 보관했습니다.'})).toHaveCount(0);await expect(verify.getByRole('button',{name:'백업 사본 검증',exact:true})).toBeDisabled();
  if(outcome==='failure')await expect(creation.getByRole('alert')).toContainText('사본 보관만 다시 시도');
 }
 expect(faults).toEqual([]);expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);
});
