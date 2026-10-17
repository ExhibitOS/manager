// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
test('hosted static publication clears token input and never requires Render or manual identifiers',async({page})=>{
 await page.addInitScript(()=>{
  Object.defineProperty(window,'isTauri',{value:true});
  Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{invoke:async(command:string,args:Record<string,unknown>)=>{
   if(command==='manager_hosted_resume')return null;
   if(command==='manager_hosted_connected')return false;
   if(command==='manager_hosted_store')return true;
   if(command==='manager_hosted_choose_source')return {title:'Synthetic exhibition',publicationId:'published-synthetic',projectName:'synthetic-show'};
   if(command==='manager_hosted_disconnect')return true;
   if(command==='manager_hosted'){const input=args.input as Record<string,unknown>;if(input.action==='accounts')return {provider:'cloudflare',action:'accounts',state:'ready',accounts:[{id:'account-a',name:'My account'}]};return {provider:'cloudflare',action:'deploy-static',state:'ready',deploymentId:'deployment-a',projectName:'synthetic-show',url:'https://synthetic-show.pages.dev'};}
   throw {code:'MANAGER_CONNECTION',guidance:'Synthetic native bridge preview only'};
  }}});
 });
 await page.goto('/');const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});
 await expect(section.getByLabel('Cloudflare API 토큰')).toBeVisible();await expect(section.getByLabel('Render API 토큰')).toHaveCount(0);
 await section.getByLabel('Cloudflare API 토큰').fill('synthetic-transient-token');await section.getByRole('button',{name:'키체인에 보관하고 연결 확인'}).click();
 await expect(section.getByLabel('Cloudflare API 토큰')).toHaveValue('');await expect(section.getByLabel('사용할 계정')).toHaveValue('account-a');
 await section.getByRole('button',{name:'공개 승인한 전시 묶음 선택'}).click();await expect(section.getByText('선택한 전시: Synthetic exhibition')).toBeVisible();
 await section.getByLabel('이 전시의 공개 권리를 확인했으며 선택한 계정에 파일을 올리는 데 동의합니다.').check();await section.getByRole('button',{name:'선택한 전시 배포'}).click();
 await expect(section.getByRole('button',{name:'공개 전시 열기'})).toBeVisible();
 expect(await page.evaluate(()=>JSON.stringify({...localStorage}))).not.toContain('synthetic-transient-token');
 await section.screenshot({path:test.info().outputPath('hosted-static-ready-light.png')});
 await page.getByLabel('화면 모드').selectOption('dark');await section.screenshot({path:test.info().outputPath('hosted-static-ready-dark.png')});
 await section.getByRole('button',{name:'연결 해제',exact:true}).click();await expect(section.getByLabel('사용할 계정')).toHaveCount(0);
});
test('browser preview offers no token entry or deployment authority',async({page})=>{
 await page.goto('/');const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});
 await expect(section.getByLabel('Cloudflare API 토큰')).toBeDisabled();await expect(section.getByRole('button',{name:'선택한 전시 배포'})).toBeDisabled();
});

test('serialized previous work survives reload and missing bundle only blocks another upload',async({page})=>{
 await page.addInitScript(()=>{
  const work=JSON.parse(JSON.stringify({provider:'cloudflare',accountId:'account-a',source:{title:'Previous public exhibition',publicationId:'publication-original',projectName:'show-a'},receipt:{provider:'cloudflare',action:'deploy-static',state:'ready',deploymentId:'old-deployment',url:'https://show-a.pages.dev'},staticReceipt:{provider:'cloudflare',action:'deploy-static',state:'ready',deploymentId:'old-deployment',url:'https://show-a.pages.dev'},apiReceipt:null,phase:'pending',sourceAvailable:false,staticOnly:true}));
  const w=window as unknown as {isTauri:boolean;hostedCommands:string[];__TAURI_INTERNALS__:{invoke:(command:string)=>Promise<unknown>}};w.isTauri=true;w.hostedCommands=[];
  w.__TAURI_INTERNALS__={invoke:async command=>{w.hostedCommands.push(command);if(command==='manager_hosted_connected')return true;if(command==='manager_hosted_resume')return work;if(command==='manager_hosted_resume_remote')return {...work,phase:'observed'};throw {code:'MANAGER_CONNECTION',guidance:'Synthetic read-only restart fixture'};}};
 });
 for(let run=0;run<2;run++){
  await page.goto('/');const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});
  await expect(section.getByRole('heading',{name:'이전 공개 작업'})).toBeVisible();await expect(section.getByText(/전시 묶음이 없거나 변경됐습니다/)).toBeVisible();await expect(section.getByRole('button',{name:'선택한 전시 배포'})).toBeDisabled();
  await section.getByRole('button',{name:'이전 배포 상태 확인'}).click();await expect(section.getByText(/저장된 작업 기록/)).toBeVisible();
  expect(await page.evaluate(()=>(window as unknown as {hostedCommands:string[]}).hostedCommands.filter(c=>c==='manager_hosted'))).toEqual([]);
  await expect(section.getByLabel('Cloudflare API 토큰')).toHaveValue('');
 }
});

test('previous-work read-only error reaches the user as a bounded diagnostic without repeating a write',async({page})=>{
 await page.addInitScript(()=>{
  const w=window as unknown as {isTauri:boolean;hostedCommands:string[];__TAURI_INTERNALS__:{invoke:(command:string)=>Promise<unknown>}};w.isTauri=true;w.hostedCommands=[];
  w.__TAURI_INTERNALS__={invoke:async command=>{w.hostedCommands.push(command);if(command==='manager_hosted_connected')return true;if(command==='manager_hosted_resume')return {provider:'cloudflare',accountId:'account-a',source:{title:'Preserved exhibition',publicationId:'publication-original',projectName:'show-a'},receipt:null,staticReceipt:null,apiReceipt:null,phase:'uncertain',sourceAvailable:false,staticOnly:true};if(command==='manager_hosted_resume_remote')throw {code:'HOSTED_CF_PROJECT_READ_HTTP_404_CODE_7003',guidance:'Synthetic safe boundary'};throw {code:'HOSTED_OPERATION_UNCERTAIN'};}};
 });
 await page.goto('/');const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});await section.getByRole('button',{name:'이전 배포 상태 확인'}).click();
 await expect(section.getByRole('alert')).toContainText('HOSTED_CF_PROJECT_READ_HTTP_404_CODE_7003');
 expect(await page.evaluate(()=>(window as unknown as {hostedCommands:string[]}).hostedCommands.filter(c=>c==='manager_hosted'))).toEqual([]);
 await expect(section.getByRole('heading',{name:'이전 공개 작업'})).toBeVisible();
});
