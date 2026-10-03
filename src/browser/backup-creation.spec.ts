// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// These synthetic IPC checks verify UI/dispatch boundaries. They do not claim native crypto or restore qualification.
for(const scenario of ['success','failure','malformed','wrong-image'] as const)test(`creation ${scenario} requires explicit consent and preserves a paused exhibition`,async({page},testInfo)=>{
 await page.addInitScript(({scenario})=>{
  type FixtureWindow=Window&{__creationInputs:unknown[];__creationCalls:string[];__resolveCreation:()=>void;__resolveStatusRefresh:()=>void};
  const fixture=window as unknown as FixtureWindow;fixture.__creationInputs=[];fixture.__creationCalls=[];
  let stopped=false;
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string,args?:{input:unknown})=>{
   fixture.__creationCalls.push(cmd);
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status'){if(stopped)await new Promise<void>(resolve=>{fixture.__resolveStatusRefresh=resolve;});return {installed:true,bundleId:'synthetic',version:'0.1.0',state:stopped?'stopped':'running',services:[],readiness:{ready:!stopped,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};}
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_create_backup'){
    fixture.__creationInputs.push(args?.input);await new Promise<void>(resolve=>{fixture.__resolveCreation=resolve;});stopped=true;
    if(scenario==='failure')throw {code:'BACKUP_ORPHAN_PENDING',guidance:'남은 helper의 작업 ID와 정확한 label을 확인하세요.'};
    return {id:'12345678-1234-1234-1234-123456789012',backupId:'23456789-1234-1234-1234-123456789012',operation:scenario==='malformed'?'created':'created-and-authenticated',files:11,image:'sha256:'+(scenario==='wrong-image'?'c':'a').repeat(64),authenticatedManifestSha256:'b'.repeat(64),writersPaused:true,at:2};
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{scenario});
 const faults:string[]=[];page.on('pageerror',e=>faults.push(e.message));await page.goto('/');
 const section=page.getByRole('region',{name:'설치된 전시 백업'}),submit=section.getByRole('button',{name:'전시를 멈추고 백업 생성'});
 await expect(section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로')).toBeEnabled();
 await section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로').fill('private key bytes');
 await section.getByLabel('백업 생성용 유지보수 이미지의 고정 ID').fill('sha256:'+'a'.repeat(64));
 await section.getByRole('checkbox',{name:'다른 앱·worker·스크립트의 DB·작품·설정 쓰기를 중지했습니다.'}).check();
 await section.getByRole('checkbox',{name:'전시가 중단되며, 결과 확인 후 직접 다시 시작하는 데 동의합니다.'}).check();
 await expect(submit).toBeDisabled();expect(await page.evaluate(()=>(window as unknown as {__creationInputs:unknown[]}).__creationInputs.length)).toBe(0);
 await section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로').fill('/private/tmp/synthetic-external-key');
 await section.getByRole('checkbox',{name:'전시가 중단되며, 결과 확인 후 직접 다시 시작하는 데 동의합니다.'}).uncheck();await expect(submit).toBeDisabled();
 await section.getByRole('checkbox',{name:'전시가 중단되며, 결과 확인 후 직접 다시 시작하는 데 동의합니다.'}).check();await expect(submit).toBeEnabled();await submit.click();
 await expect(section.getByRole('progressbar',{name:'백업 생성과 인증 진행 중'})).toBeVisible();
 await expect(page.getByRole('region',{name:'실제 전시 상태'}).getByText('확인되지 않음').first()).toBeVisible();
 for(const name of ['전시 설치','시작','정지','재시작','전시 열기','실패한 작업 다시 시도','백업 사본 검증'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await expect(section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로')).toBeDisabled();
 await section.locator('form').dispatchEvent('submit');
 expect(await page.evaluate(()=>(window as unknown as {__creationInputs:unknown[]}).__creationInputs)).toEqual([{image:'sha256:'+'a'.repeat(64),keyPath:'/private/tmp/synthetic-external-key',externalWritersQuiesced:true,downtimeAccepted:true}]);
 await page.evaluate(()=>(window as unknown as {__resolveCreation:()=>void;__resolveStatusRefresh:()=>void}).__resolveCreation());
 await expect(section.getByRole('checkbox').first()).not.toBeChecked();await expect(section.getByRole('checkbox').last()).not.toBeChecked();await expect(submit).toBeDisabled();
 if(scenario==='success'){
  await expect(section.getByRole('heading',{name:'암호화 백업 생성과 인증을 완료했습니다.'})).toBeVisible();await expect(section).toContainText('11개 파일');await expect(section).toContainText('전시는 정지 상태로 남습니다. 데이터 복원은 실행하지 않았습니다.');
  await section.getByText('생성 식별 정보',{exact:true}).click();await expect(section.getByText('b'.repeat(64),{exact:true})).toBeVisible();await expect(section).toContainText('backup-creation-12345678-1234-1234-1234-123456789012/archive/');
 }else{
  await expect(section.getByRole('alert')).toBeFocused();await expect(section.getByRole('heading',{name:'암호화 백업 생성과 인증을 완료했습니다.'})).toHaveCount(0);await expect(section.getByRole('alert')).toContainText('원본과 키, 새 사본·작업 공간은 보존');
 }
 for(const name of ['전시 설치','시작','재시작','실패한 작업 다시 시도'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await page.evaluate(()=>(window as unknown as {__resolveStatusRefresh:()=>void}).__resolveStatusRefresh());
 await expect(page.getByRole('button',{name:'시작',exact:true})).toBeEnabled();
 expect(await page.evaluate(()=>(window as unknown as {__creationCalls:string[]}).__creationCalls.filter(c=>c==='manager_action'))).toEqual([]);
 expect(await page.evaluate(()=>Object.keys(localStorage).concat(Object.keys(sessionStorage)))).toEqual([]);
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  expect(await section.locator('input:not([type="checkbox"])').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44))).toBe(true);
  expect(await section.locator('.backup-ack').evaluateAll(es=>es.every(e=>e.getBoundingClientRect().height>=44))).toBe(true);
  await section.screenshot({path:`/private/tmp/exhibitos-manager-creation-${testInfo.project.name}-${scenario}-${width}.png`});
 }
 if(scenario==='success'){
  await section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로').fill('/private/tmp/another-synthetic-key');await expect(section.getByRole('heading',{name:'암호화 백업 생성과 인증을 완료했습니다.'})).toHaveCount(0);
 }
 expect(faults).toEqual([]);
});

for(const state of ['running','interrupted','malformed'] as const)test(`persisted ${state} backup history never invents completion or repeats mutations`,async({page})=>{
 await page.addInitScript(({state})=>{
  Object.assign(window,{isTauri:true,__historyFixed:false,__historyMutations:0,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs')return [];
   if(cmd==='manager_backup_jobs')return (window as unknown as {__historyFixed:boolean}).__historyFixed?[]:[{id:'12345678-1234-1234-1234-123456789012',operation:'create',state:state==='malformed'?'completed':state,stage:'pausing-writers',errorCode:state==='interrupted'?'INTERRUPTED':null,createdAt:1,updatedAt:2}];
   (window as unknown as {__historyMutations:number}).__historyMutations++;throw Error('Unexpected mutation');
  }}});
 },{state});
 await page.goto('/');const history=page.getByRole('region',{name:'저장된 백업 생성 기록'});
 if(state==='malformed'){
  await expect(page.getByRole('complementary',{name:'다음 할 일'})).toContainText('백업 기록');
  await expect(history.getByRole('alert')).toContainText('완료 상태를 추측하지 않습니다.');await expect(page.getByRole('button',{name:'시작',exact:true})).toBeDisabled();await expect(page.getByRole('button',{name:'백업 사본 검증',exact:true})).toBeDisabled();
  await page.evaluate(()=>{(window as unknown as {__historyFixed:boolean}).__historyFixed=true;});await page.getByRole('button',{name:'실행 도구 다시 확인'}).click();await expect(history).toContainText('저장된 생성 기록이 없습니다.');await expect(page.getByRole('button',{name:'시작',exact:true})).toBeEnabled();
 }else{
  await expect(history).toContainText('전시 쓰기 중지');await expect(history).toContainText(state==='running'?'진행 중':'중단됨');
  if(state==='running'){for(const name of ['시작','재시작','백업 사본 검증','전시를 멈추고 백업 생성'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();}
  else{await expect(history).toContainText('완료된 사본으로 표시하지 않습니다.');await expect(history).toContainText('정확한 label');}
  await page.reload();await expect(page.getByRole('region',{name:'저장된 백업 생성 기록'})).toContainText(state==='running'?'진행 중':'중단됨');
 }
 expect(await page.evaluate(()=>(window as unknown as {__historyMutations:number}).__historyMutations)).toBe(0);
});


test('a pre-backup status request cannot resurrect stale ready state during creation',async({page})=>{
 await page.addInitScript(()=>{
  type FixtureWindow=Window&{__oldStatus:()=>void;__finishBackup:()=>void};const fixture=window as unknown as FixtureWindow;
  let reads=0,finished=false;
  const status=()=>({installed:true,bundleId:'synthetic',version:'0.1.0',state:finished?'stopped':'running',services:[],readiness:{ready:!finished,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null});
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status'){reads++;const old=status();if(reads===2)await new Promise<void>(resolve=>{fixture.__oldStatus=resolve;});return old;}
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(cmd==='manager_jobs'||cmd==='manager_logs'||cmd==='manager_backup_jobs')return [];
   if(cmd==='manager_create_backup'){await new Promise<void>(resolve=>{fixture.__finishBackup=resolve;});finished=true;return {id:'12345678-1234-1234-1234-123456789012',backupId:'23456789-1234-1234-1234-123456789012',operation:'created-and-authenticated',files:11,image:'sha256:'+'a'.repeat(64),authenticatedManifestSha256:'b'.repeat(64),writersPaused:true,at:2};}
   throw Error('Unexpected command');
  }}});
 });
 await page.goto('/');const section=page.getByRole('region',{name:'설치된 전시 백업'}),state=page.getByRole('region',{name:'실제 전시 상태'});
 await expect(section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로')).toBeEnabled();
 await section.getByLabel('생성에 사용할 외부 키 파일의 전체 경로').fill('/private/tmp/synthetic-key');await section.getByLabel('백업 생성용 유지보수 이미지의 고정 ID').fill('sha256:'+'a'.repeat(64));
 for(const checkbox of await section.getByRole('checkbox').all())await checkbox.check();
 await page.getByRole('button',{name:'실행 도구 다시 확인'}).click();
 await section.getByRole('button',{name:'전시를 멈추고 백업 생성'}).click();await expect(section.getByRole('progressbar')).toBeVisible();
 await page.evaluate(()=>(window as unknown as {__oldStatus:()=>void}).__oldStatus());
 await expect(state.getByText('확인되지 않음').first()).toBeVisible();await expect(state.getByText('관람 준비 완료')).toHaveCount(0);
 await page.evaluate(()=>(window as unknown as {__finishBackup:()=>void}).__finishBackup());
 await expect(section.getByRole('heading',{name:'암호화 백업 생성과 인증을 완료했습니다.'})).toBeVisible();await expect(state.getByText('정지됨',{exact:true})).toBeVisible();
});
