// SPDX-License-Identifier: Apache-2.0
import {test,expect,type Page} from '@playwright/test';

// Synthetic IPC only: no provider request, credential store, native app or key.
async function fixture(page:Page,{fail=false,hold=false}={}){
 await page.addInitScript(({fail,hold})=>{
  const selectedSource={title:'Preserved static exhibition',publicationId:'original-publication',projectName:'original-show'};
  const staticReceipt={provider:'cloudflare',action:'deploy-static',state:'ready',projectName:'original-show',deploymentId:'original-deployment',url:'https://original-show.pages.dev'};
  const calls:Record<string,unknown>[]=[];let connected=false,contextReads=0;
  Object.assign(window,{isTauri:true,__reconnectCalls:calls,__reconnectContextReads:()=>contextReads,__releaseReconnect:()=>{}});
  Object.assign(window,{__TAURI_INTERNALS__:{invoke:async(command:string,args?:{input?:Record<string,unknown>})=>{
   if(command==='manager_update_preparation_startup')return {controllerBlocked:false,canOpenManager:false,job:null};
   if(command==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-profile/local-runtime',available:true}],errorCode:null};
   if(command==='manager_status')return {installed:true,bundleId:'synthetic',version:'0.1.0',state:'stopped',services:[],readiness:{ready:false,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
   if(['manager_detect','manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(command))return [];
   if(command==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(command==='manager_maintenance_context')return null;
   if(command==='manager_hosted_resume')return {provider:'cloudflare',accountId:'account-original',source:selectedSource,staticReceipt,phase:'observed',sourceAvailable:true,staticOnly:true};
   if(command==='manager_hosted_connected')return true;
   if(command==='manager_hosted_recovery'){
    if(args?.input?.action==='context')contextReads++;
    return {source:connected?{workspaceId:'tea-existing',serviceId:'srv-existing',postgresId:'dpg-existing'}:null,jobs:[],restoreChecks:[],keySaved:false,archiveSaved:null,publicationSaved:null,publicationCopies:[],remoteTargets:[],remoteRestores:[]};
   }
   if(command==='manager_hosted'){
    const input=args?.input??{};calls.push(structuredClone(input));
    if(input.action==='accounts')return {provider:'render',action:'accounts',state:'ready',accounts:[{id:'tea-existing',name:'Existing workspace'}]};
    if(input.action==='services')return {provider:'render',action:'services',state:'ready',services:[{id:'srv-existing',name:'Existing API',free:true}],postgres:[{id:'dpg-existing',name:'Existing database',free:true}]};
    if(input.action==='connect-existing'){
     if(hold)await new Promise<void>(resolve=>Object.assign(window,{__releaseReconnect:resolve}));
     if(fail)throw {code:'HOSTED_RESOURCE_CONFLICT'};
     connected=true;return {provider:'render',action:'connect-existing',state:'ready',serviceId:'srv-existing',postgresId:'dpg-existing',projectName:'original-show',url:'https://original-show.onrender.com'};
    }
    throw {code:'UNEXPECTED_DEPLOYMENT_REQUEST'};
   }
   throw {code:'MANAGER_CONNECTION',guidance:'Synthetic preview only'};
  }}});
 },{fail,hold});
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();
 const section=page.getByRole('region',{name:'전시를 웹에 공개하기'});
 await expect(section.getByText('선택한 전시: Preserved static exhibition')).toBeVisible();
 await section.getByRole('radio',{name:'API·데이터베이스 준비 · 추가 연결 필요'}).check();
 await section.getByRole('group',{name:'Render 연결',exact:true}).getByRole('button',{name:'보관한 토큰으로 연결 확인'}).click();
 await expect(section.getByLabel('사용할 워크스페이스')).toHaveValue('tea-existing');
 await section.getByRole('button',{name:'사용할 API·데이터베이스 확인'}).click();
 await section.getByRole('combobox',{name:'API 서버',exact:true}).selectOption('srv-existing');
 await section.getByRole('combobox',{name:'데이터베이스',exact:true}).selectOption('dpg-existing');
 const connect=section.getByRole('button',{name:'기존 백업 대상 연결',exact:true});
 await expect(connect).toBeDisabled();
 await section.getByLabel('원래 배포 프로젝트 이름').fill('  original-show  ');
 await expect(connect).toBeEnabled();
 return {section,connect};
}
const calls=(page:Page)=>page.evaluate(()=>(window as unknown as {__reconnectCalls:Record<string,unknown>[]}).__reconnectCalls);
const connection={provider:'render',action:'connect-existing',workspaceId:'tea-existing',serviceId:'srv-existing',postgresId:'dpg-existing',projectName:'original-show'};

test('explicit existing connection sends only exact selected binding and refreshes recovery context',async({page})=>{
 const {section,connect}=await fixture(page);
 await connect.click();await expect(section.getByText(/백업 대상 연결을 저장했습니다/)).toBeVisible();
 expect((await calls(page)).filter(c=>c.action==='connect-existing')).toEqual([connection]);
 expect((await calls(page)).some(c=>String(c.action).startsWith('deploy'))).toBe(false);
 await section.getByText('서버 백업 관리',{exact:true}).click();
 await section.getByRole('button',{name:'배포한 서버의 백업 준비'}).click();
 await expect(section.getByText('기존 배포 기록의 서버와 DB를 선택했어요.',{exact:false})).toBeVisible();
 expect(await page.evaluate(()=>(window as unknown as {__reconnectContextReads:()=>number}).__reconnectContextReads())).toBe(1);
});

test('connection failure retains selected exhibition and static result and requires explicit retry',async({page})=>{
 const {section,connect}=await fixture(page,{fail:true});
 await connect.click();await expect(section.getByRole('alert')).toContainText('HOSTED_RESOURCE_CONFLICT');
 await expect(section.getByRole('button',{name:'공개 전시 열기'})).toBeVisible();
 await expect(section.getByText('선택한 전시: Preserved static exhibition')).toBeVisible();
 await expect(section.getByRole('combobox',{name:'API 서버',exact:true})).toHaveValue('srv-existing');
 await expect(section.getByRole('combobox',{name:'데이터베이스',exact:true})).toHaveValue('dpg-existing');
 await expect(section.getByLabel('원래 배포 프로젝트 이름')).toHaveValue('  original-show  ');
 expect((await calls(page)).filter(c=>c.action==='connect-existing')).toEqual([connection]);
 await expect(connect).toBeEnabled();await connect.click();
 await expect.poll(async()=>(await calls(page)).filter(c=>c.action==='connect-existing').length).toBe(2);
 expect((await calls(page)).some(c=>String(c.action).startsWith('deploy'))).toBe(false);
});

test('pending existing connection disables repeated submission without creating resources',async({page})=>{
 const {section,connect}=await fixture(page,{hold:true});
 await connect.click();await expect(connect).toBeDisabled();
 await connect.evaluate(button=>(button as HTMLButtonElement).click());
 expect((await calls(page)).filter(c=>c.action==='connect-existing')).toEqual([connection]);
 expect((await calls(page)).some(c=>String(c.action).startsWith('deploy'))).toBe(false);
 await page.evaluate(()=>(window as unknown as {__releaseReconnect:()=>void}).__releaseReconnect());
 await expect(section.getByText(/백업 대상 연결을 저장했습니다/)).toBeVisible();
});
