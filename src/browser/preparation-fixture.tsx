// SPDX-License-Identifier: Apache-2.0
// Browser-only test parent: production components, parsers and IPC are imported without replacement.
import {useEffect,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {PreparationStartup} from '../PreparationStartup';
import {UpdatePreparation} from '../UpdatePreparation';
import {AppearanceControl} from '../AppearanceControl';
type FixtureWindow=Window&{managerMounted?:number;mountHash?:string};
export function mountPreparationFixture(element:HTMLElement,token:string,installationId:string){
 function Host(){
  const [selection,setSelection]=useState(token),[active,setActive]=useState(false);
  useEffect(()=>{const fixture=window as FixtureWindow;fixture.managerMounted=(fixture.managerMounted??0)+1;fixture.mountHash=window.location.hash;},[]);
  return <main><h1>Manager 테스트 부모</h1><AppearanceControl/><button onClick={()=>setSelection('99999999-1234-4234-8234-123456789012')}>테스트 관리 공간 전환</button><UpdatePreparation native selectionToken={selection} installationId={installationId} active={active} onActivity={setActive}/></main>;
 }
 createRoot(element).render(<PreparationStartup><Host/></PreparationStartup>);
}
