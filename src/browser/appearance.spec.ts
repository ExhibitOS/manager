// SPDX-License-Identifier: Apache-2.0
import {test,expect} from '@playwright/test';
test('appearance follows the system, persists an override and keeps task navigation accessible',async({page})=>{
 await page.emulateMedia({colorScheme:'dark'});
 await page.goto('/');
 const picker=page.getByRole('combobox',{name:'화면 모드'});
 await expect(picker).toHaveValue('system');
 await expect(page.locator('html')).toHaveAttribute('data-appearance','dark');
 await picker.selectOption('light');
 await expect(page.locator('html')).toHaveAttribute('data-appearance','light');
 await page.reload();
 await expect(picker).toHaveValue('light');
 await expect(page.locator('html')).toHaveAttribute('data-appearance','light');
 await picker.selectOption('dark');
 await expect(page.locator('html')).toHaveAttribute('data-appearance','dark');
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await page.screenshot({path:test.info().outputPath(`manager-dark-${width}.png`),fullPage:true});
 }
 const navigation=page.getByRole('navigation',{name:'관리 메뉴'});
 await navigation.getByRole('link',{name:'백업·복원',exact:true}).click();
 await expect(page).toHaveURL(/#backup$/);
 await expect(page.getByRole('heading',{name:'백업에서 새 전시 복원'})).toBeVisible();
 await picker.selectOption('system');
 await page.emulateMedia({colorScheme:'light'});
 await expect(page.locator('html')).toHaveAttribute('data-appearance','light');
 await page.screenshot({path:test.info().outputPath('manager-light.png'),fullPage:true});
});

test('both appearances retain readable semantic text, primary controls and warnings',async({page})=>{
 await page.goto('/');
 for(const mode of ['light','dark']){
  await page.getByRole('combobox',{name:'화면 모드'}).selectOption(mode);
  const ratios=await page.evaluate(()=>{
   const styles=getComputedStyle(document.documentElement);
   const luminance=(name:string)=>{const hex=styles.getPropertyValue(name).trim().slice(1);const full=hex.length===3?hex.split('').map(c=>c+c).join(''):hex;const [r,g,b]=[0,2,4].map(i=>{const v=parseInt(full.slice(i,i+2),16)/255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;});return r!*.2126+g!*.7152+b!*.0722;};
   return [['--text','--surface'],['--secondary','--canvas'],['--on-accent','--accent'],['--warning-text','--warning-surface']].map(([a,b])=>{const l1=luminance(a!),l2=luminance(b!);return (Math.max(l1,l2)+.05)/(Math.min(l1,l2)+.05);});
  });
  for(const ratio of ratios)expect(ratio).toBeGreaterThanOrEqual(4.5);
 }
});

test('task navigation stays visible and does not cover linked headings or keyboard focus',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'고급 설정',exact:true}).click();
 const nav=page.getByRole('navigation',{name:'관리 메뉴'});
 for(const width of [320,640,1120]){
  await page.setViewportSize({width,height:900});
  await nav.getByRole('link',{name:'작업 기록',exact:true}).click();
  await page.locator('#history').scrollIntoViewIfNeeded();
  const pinned=await nav.boundingBox();
  expect(pinned!.y).toBeGreaterThanOrEqual(0);expect(pinned!.y).toBeLessThan(24);
  for(const [name,id] of [['전시','exhibition'],['백업·복원','backup'],['업데이트','updates'],['작업 기록','history'],['배포 준비','deployment']]){
   await nav.getByRole('link',{name:name!,exact:true}).click();
   await expect(page).toHaveURL(new RegExp(`#${id}$`));
   const heading=await page.locator(`#${id}`).getByRole('heading').first().boundingBox();
   const menu=await nav.boundingBox();
   expect(heading!.y).toBeGreaterThanOrEqual(menu!.y+menu!.height);
   expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  }
  await page.getByRole('link',{name:'전시 관리로 건너뛰기'}).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('region',{name:'전시 설치와 실행'})).toBeFocused();
  const controls=await page.locator('#controls').boundingBox(),menu=await nav.boundingBox();
  expect(controls!.y).toBeGreaterThanOrEqual(menu!.y+menu!.height);
 }
});
