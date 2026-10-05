// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Delayed synthetic IPC checks feedback timing, not native Engine completion.
for(const action of ['install','start','stop','restart','failure'] as const)test(`delayed ${action} shows immediate progress beside controls`,async({page})=>{
 await page.addInitScript(({action})=>{
  const installed=action!=='install',ready=action==='stop'||action==='restart';
  const status={installed,bundleId:'synthetic',version:'0.1.0',state:ready?'running':'stopped',services:[],readiness:{ready,version:null,protocolVersion:null,errorCode:null},storage:{usedBytes:null,freeBytes:10000000000,minimumFreeBytes:0},activeJob:null};
  Object.assign(window,{isTauri:true,__finishOperation:null,__operationCount:0,__TAURI_INTERNALS__:{invoke:async(cmd:string)=>{
   if(cmd==='manager_installations')return {selectionToken:'45678901-1234-1234-1234-123456789012',activeId:'56789012-1234-1234-1234-123456789012',mode:'managed',installations:[{id:'56789012-1234-1234-1234-123456789012',kind:'default',createdAt:1,path:'/private/tmp/synthetic-progress/local-runtime',available:true}],errorCode:null};
   if(cmd==='manager_status')return status;
   if(cmd==='manager_detect')return [{kind:'docker',installed:true,available:true,engineVersion:'synthetic',composeVersion:'synthetic',errorCode:null,guidance:null}];
   if(['manager_jobs','manager_logs','manager_backup_jobs','manager_helper_reconciliations','manager_maintenance_retries'].includes(cmd))return [];
   if(cmd==='manager_restoration_context')return {fresh:false,job:null,receipt:null};
   if(cmd==='manager_maintenance_context')return null;
   if(cmd==='manager_install'||cmd==='manager_action'){
    const harness=window as unknown as {__operationCount:number;__finishOperation:()=>void};harness.__operationCount++;
    return new Promise((resolve,reject)=>{harness.__finishOperation=()=>{
     if(action==='failure'){reject({code:'ENGINE_PERMISSION',guidance:'실행 도구 접근 권한을 확인하세요.'});return;}
     status.installed=true;status.readiness.ready=action!=='install'&&action!=='stop';status.state=status.readiness.ready?'running':'stopped';
     resolve({id:'synthetic-progress',action,state:'completed',attempt:1,progress:100,createdAt:1,updatedAt:2,errorCode:null,guidance:null});
    };});
   }
   throw Error('Unexpected synthetic command');
  }}});
 },{action});
 await page.goto('/');const controls=page.getByRole('region',{name:'전시 설치와 실행'});await expect(page.getByRole('alert')).toHaveCount(0);
 const label=action==='install'?'전시 설치':action==='stop'?'정지':action==='restart'?'재시작':'시작';
 await expect(controls.getByRole('button',{name:label,exact:true})).toBeEnabled();
 await controls.getByRole('button',{name:label,exact:true}).click();
 const feedback=controls.getByRole('status');await expect(feedback).toContainText('작업 진행 중');
 const progress=feedback.getByRole('progressbar');await expect(progress).toBeVisible();await expect(progress).not.toHaveAttribute('value');
 if(action==='start')await controls.screenshot({path:'/private/tmp/exhibitos-manager-progress-'+test.info().project.name+'.png'});
 await expect(controls.getByRole('button',{name:action==='install'?'설치 중…':action==='stop'?'정지 중…':action==='restart'?'재시작 중…':'시작 중…',exact:true})).toBeDisabled();
 expect(await page.evaluate(()=>(window as unknown as {__operationCount:number}).__operationCount)).toBe(1);
 for(const width of [320,1120]){await page.setViewportSize({width,height:900});expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);}
 await page.evaluate(()=>(window as unknown as {__finishOperation:()=>void}).__finishOperation());
 await expect(progress).toHaveCount(0);
 await expect(feedback).toContainText(action==='failure'?'완료하지 못했습니다':'작업이 완료됐습니다');
 if(action==='failure')await expect(feedback).toContainText('ENGINE_PERMISSION');
});
