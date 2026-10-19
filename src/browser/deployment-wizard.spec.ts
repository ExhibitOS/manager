// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
const image='ghcr.io/exhibitos/platform@sha256:'+'a'.repeat(64);
test('deployment preparation validates local/server/cloud, clears stale results and falls back without executing',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();await page.getByText('기존 서버 배포 도구',{exact:true}).click();const panel=page.getByRole('region',{name:'배포 준비'}),prepare=panel.getByRole('button',{name:'입력 확인·파일 준비'});
 await prepare.click();await expect(panel.getByRole('alert')).toBeVisible();await expect(panel.getByRole('button',{name:'준비 파일 저장'})).toHaveCount(0);
 await panel.getByLabel('검증한 실행 이미지 (저장소 digest 또는 로컬 이미지 ID)').fill(image);await prepare.click();
 await expect(panel.getByText('준비 파일 생성됨 · 배포 미실행',{exact:true})).toBeVisible();
 await panel.locator('summary').filter({hasText:'파일 내용 확인 (비밀값 없음)'}).click();
 let text=await panel.locator('pre').innerText();expect(JSON.parse(text).target).toEqual({type:'local-compose'});
 await panel.getByLabel('배포 장소').selectOption('server');await expect(panel.locator('pre')).toHaveCount(0);
 await panel.getByLabel('서버 주소').fill('host.example.invalid');await panel.getByLabel('서버 사용자 (root 제외)').fill('root');await prepare.click();await expect(panel.getByRole('alert')).toBeVisible();
 await panel.getByLabel('서버 사용자 (root 제외)').fill('exhibitos');await prepare.click();await panel.locator('summary').filter({hasText:'파일 내용 확인 (비밀값 없음)'}).click();
 text=await panel.locator('pre').innerText();const server=JSON.parse(text);expect(server.target).toEqual({type:'generic-ssh',host:'host.example.invalid',user:'exhibitos',port:22,sshKeySecretRef:'ssh-key'});expect(server.runtime).toEqual({image,httpPort:13200,replicas:1});expect(server.storage.metadata.host).toBe('database');expect(server.secretRefs).toHaveLength(2);
 const pending=page.waitForEvent('download');await panel.getByRole('button',{name:'준비 파일 저장'}).click();const download=await pending;expect(download.suggestedFilename()).toBe('exhibitos-deployment.oed.json');
 const stream=await download.createReadStream();const decoder=new TextDecoder();let downloaded='';for await(const part of stream!)downloaded+=decoder.decode(part,{stream:true});downloaded+=decoder.decode();expect(JSON.parse(downloaded)).toEqual(server);
 await panel.getByLabel('서버 주소').fill('changed.example.invalid');await expect(panel.getByRole('button',{name:'준비 파일 저장'})).toHaveCount(0);
 await panel.getByLabel('배포 장소').selectOption('cloud');await prepare.click();await expect(panel.getByRole('alert')).toBeVisible();
 for(const [label,value] of [['AWS 계정 번호 (12자리)','123456789012'],['리전','eu-central-1'],['격리된 프로젝트 이름','exhibition-test'],['확인한 AMI ID','ami-12345678'],['기존 서브넷 ID','subnet-12345678'],['기존 보안 그룹 ID (쉼표로 구분)','sg-12345678'],['기존 SSH 공개 키 등록 이름','exhibition-key']])await panel.getByLabel(label!).fill(value!);
 await prepare.click();await expect(panel.getByText(/월 비용 미확인/)).toBeVisible();await expect(panel.getByText(/새 지출 허용은 0 KRW/)).toBeVisible();
 await panel.getByRole('button',{name:'보유 서버로 직접 배포 준비'}).click();await expect(panel.getByLabel('배포 장소')).toHaveValue('server');await expect(panel.getByRole('button',{name:'준비 파일 저장'})).toHaveCount(0);
 await panel.getByLabel('안내 수준').selectOption('advanced');await expect(panel.getByText(/OED 1.0.0-draft.1의 현재 지원 범위/)).toBeVisible();
});
