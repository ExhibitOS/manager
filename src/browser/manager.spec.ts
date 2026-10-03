// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
// Browser-only checks deliberately do not mock a native lifecycle engine.
test('browser preview reports no native authority and cannot invoke lifecycle operations',async({page})=>{
 await page.goto('/');await expect(page.getByRole('heading',{name:'전시를 시작할 준비'})).toBeVisible();await expect(page.getByText('데스크톱 앱에서 열어주세요.')).toBeVisible();
 for(const name of ['전시 설치','시작','정지','재시작','전시 열기','백업 사본 검증'])await expect(page.getByRole('button',{name,exact:true})).toBeDisabled();
 await expect(page.getByLabel('별도로 보관한 키 파일의 전체 경로')).toBeDisabled();
 await expect(page.getByText('측정할 수 없음',{exact:true})).toBeVisible();
 await page.keyboard.press('Tab');await expect(page.getByRole('link',{name:'전시 관리로 건너뛰기'})).toBeFocused();
 const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth);expect(overflow).toBe(false);
});
