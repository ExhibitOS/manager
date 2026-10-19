// SPDX-License-Identifier: Apache-2.0
import {useEffect,useState} from 'react';
type Appearance='system'|'light'|'dark';
const storageKey='exhibitos.manager.appearance';
function savedAppearance():Appearance{
 try{const value=localStorage.getItem(storageKey);return value==='light'||value==='dark'?value:'system';}catch{return 'system';}
}
// A presentation preference only: no lifecycle operation or authentication state changes.
export function AppearanceControl(){
 const [appearance,setAppearance]=useState<Appearance>(savedAppearance);
 useEffect(()=>{
  const media=window.matchMedia('(prefers-color-scheme: dark)');
  const apply=()=>{document.documentElement.dataset.appearance=appearance==='system'?(media.matches?'dark':'light'):appearance;};
  apply();media.addEventListener('change',apply);
  return()=>media.removeEventListener('change',apply);
 },[appearance]);
 return <label className="appearance-control">화면 모드<select aria-label="화면 모드" value={appearance} onChange={event=>{const value=event.target.value as Appearance;setAppearance(value);try{localStorage.setItem(storageKey,value);}catch{/* This window still applies the preference. */}}}><option value="system">시스템 설정 따르기</option><option value="light">라이트</option><option value="dark">다크</option></select></label>;
}
