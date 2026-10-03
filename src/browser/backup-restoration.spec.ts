// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Synthetic IPC verifies UI boundaries; actual Docker recovery evidence is separate.
for(const scenario of ['success','failure','malformed','wrong-port'] as const)test(`fresh restoration ${scenario} requires consent and coherent post-operation status`,async({page},testInfo)=>{
 await page.addInitScript(({scenario})=>{
  type Fixture=Window&{__restoreInputs:unknown[];__restoreResolve:()=>void;__restoreRefresh:()=>void;__restoreCalls:string[]};
  const f=window as unknown as Fixture;f.__restoreInputs=[];f.__restoreCalls=[];
  let attempted=false;
  const receipt={id:'12345678-1234-1234-1234-123456789012',operation:'restored-and-running',backupId:'23456789-1234-1234-1234-123456789012',authenticatedManifestSha256:'b'.repeat(64),bundleId:'34567890-1234-1234-1234-123456789012',projectName:'exhibitos-34567890-1234-1234-1234-123456789012',openUrl:'http://127.0.0.1:4500',at:2};
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input:unknown})=>{
   f.__restoreCalls.push(cmd);
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status'){if(attempted)await new Promise<void>(resolve=>{f.__restoreRefresh=resolve;});return {installed:attempted&&scenario!=='failure',bundleId:attempted?receipt.bundleId:null,version:null,state:attempted?'running':'not_installed',services:[],readiness:{ready:attempted&&scenario!=='failure',version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};}
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_restoration_context')return attempted?{fresh:false,job:{id:receipt.id,state:scenario==='failure'?'failed':'completed',stage:scenario==='failure'?'restoring-and-verifying':'complete',errorCode:scenario==='failure'?'ENGINE_OPERATION_FAILED':null,createdAt:1,updatedAt:2},receipt:scenario==='failure'?null:receipt}:{fresh:true,job:null,receipt:null};
   if(cmd==='manager_restore_backup'){
    f.__restoreInputs.push(args?.input);await new Promise<void>(resolve=>{f.__restoreResolve=resolve;});attempted=true;
    if(scenario==='failure')throw {code:'RESTORE_FAILED',guidance:'새 후보와 원본을 보존하고 복원 기록을 확인하세요.'};
    return {...receipt,operation:scenario==='malformed'?'verified':'restored-and-running',openUrl:scenario==='wrong-port'?'http://127.0.0.1:4501':receipt.openUrl};
   }
   throw Error('Unexpected synthetic mutation');
  }}});
 },{scenario});
 const faults:string[]=[];page.on('pageerror',e=>faults.push(e.message));await page.goto('/');
 const region=page.getByRole('region',{name:'백업에서 새 전시 복원'}),submit=region.getByRole('button',{name:'새 전시 복원과 시작'});
 await expect(region.getByLabel('복원에 사용할 외부 키 파일의 전체 경로')).toBeEnabled();
 await region.getByLabel('복원할 암호화 백업 폴더의 전체 경로').fill('/private/tmp/synthetic-archive');
 await region.getByLabel('복원에 사용할 외부 키 파일의 전체 경로').fill('key contents');
 await region.getByLabel('복원용 유지보수 이미지의 고정 ID').fill('sha256:'+'a'.repeat(64));
 const ack=region.getByRole('checkbox');await ack.check();await expect(submit).toBeDisabled();
 await region.getByLabel('복원에 사용할 외부 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');
 await region.getByLabel('새 전시의 로컬 포트').fill('80');await expect(submit).toBeDisabled();
 await region.getByLabel('새 전시의 로컬 포트').fill('4500');await ack.uncheck();await expect(submit).toBeDisabled();
 await ack.check();await expect(submit).toBeEnabled();await submit.click();
 await expect(region.getByRole('progressbar',{name:'새 전시 복원 진행 중'})).toBeVisible();
 for(const name of ['전시 설치','시작','정지','재시작','전시 열기','백업 사본 검증','전시를 멈추고 백업 생성'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await region.locator('form').dispatchEvent('submit');
 expect(await page.evaluate(()=>(window as unknown as {__restoreInputs:unknown[]}).__restoreInputs)).toEqual([{image:'sha256:'+'a'.repeat(64),keyPath:'/private/tmp/synthetic-key',sourcePath:'/private/tmp/synthetic-archive',port:4500,freshInstallationAccepted:true}]);
 await page.evaluate(()=>(window as unknown as {__restoreResolve:()=>void}).__restoreResolve());
 await expect(ack).not.toBeChecked();await expect(submit).toBeDisabled();
 if(scenario==='success')await expect(region.getByRole('heading',{name:'새 전시 복원과 시작을 확인했습니다.'})).toBeVisible();
 else {await expect(region.getByRole('alert')).toBeFocused();await expect(region.getByRole('heading',{name:'새 전시 복원과 시작을 확인했습니다.'})).toHaveCount(0);}
 await expect(page.getByRole('button',{name:'전시 열기',exact:true})).toBeDisabled();
 await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__restoreRefresh:()=>void}).__restoreRefresh());
 if(scenario==='failure')await expect(region.getByText('복원 실패',{exact:true})).toBeVisible();
 else {await expect(page.getByRole('button',{name:'전시 열기',exact:true})).toBeEnabled();await expect(region.getByText('복원 완료 기록',{exact:true})).toBeVisible();}
 await expect(submit).toBeDisabled();
 expect(await page.evaluate(()=>(window as unknown as {__restoreCalls:string[]}).__restoreCalls.filter(c=>c==='manager_action'||c==='manager_install'))).toEqual([]);
 expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);
 for(const width of [320,640,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await region.screenshot({path:`/private/tmp/exhibitos-manager-restoration-${testInfo.project.name}-${scenario}-${width}.png`});}
 expect(faults).toEqual([]);
});

for(const state of ['completed','failed','interrupted','running','malformed','unavailable'] as const)test(`persisted restoration ${state} fences unsafe writes and never invents success`,async({page})=>{
 await page.addInitScript(({state})=>{
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_helper_reconciliations')return [];
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:null,state:'running',services:[],readiness:{ready:true,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_restoration_context'){
    if(state==='unavailable')throw {code:'STATE_UNAVAILABLE',guidance:'기록을 확인하세요.'};
    const receipt={id:'12345678-1234-1234-1234-123456789012',operation:'restored-and-running',backupId:'23456789-1234-1234-1234-123456789012',authenticatedManifestSha256:'b'.repeat(64),bundleId:'34567890-1234-1234-1234-123456789012',projectName:'exhibitos-34567890-1234-1234-1234-123456789012',openUrl:'http://127.0.0.1:4500',at:2};
    return {fresh:false,job:{id:receipt.id,state:state==='malformed'?'completed':state,stage:state==='completed'||state==='malformed'?'complete':'restoring-and-verifying',errorCode:['failed','interrupted'].includes(state)?'INTERRUPTED':null,createdAt:1,updatedAt:2},receipt:state==='completed'?receipt:null};
   }
   throw Error('Unexpected mutation');
  }}});
 },{state});await page.goto('/');
 const region=page.getByRole('region',{name:'백업에서 새 전시 복원'});
 if(state==='completed'){await expect(region.getByRole('heading',{name:'새 전시 복원과 시작을 확인했습니다.'})).toBeVisible();await expect(page.getByRole('button',{name:'재시작',exact:true})).toBeEnabled();}
 else {
  if(state==='malformed'||state==='unavailable')await expect(region.getByText('저장된 복원 상태를 확인할 수 없습니다.',{exact:true})).toBeVisible();
  else await expect(region.getByText({failed:'복원 실패',interrupted:'복원 중단',running:'복원 진행 중'}[state],{exact:true})).toBeVisible();
  await expect(region.getByRole('heading',{name:'새 전시 복원과 시작을 확인했습니다.'})).toHaveCount(0);
  await expect(page.getByRole('region',{name:'실제 전시 상태'}).getByText('복원 상태 확인 필요',{exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'전시 열기',exact:true})).toBeDisabled();
  for(const name of ['전시 설치','시작','재시작','백업 사본 검증','전시를 멈추고 백업 생성'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
  if(state!=='running')await expect(page.getByRole('button',{name:'정지',exact:true})).toBeEnabled();
 }
 await expect(region.getByRole('button',{name:'새 전시 복원과 시작'})).toBeDisabled();
});
