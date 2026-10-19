// SPDX-License-Identifier: Apache-2.0
import {createRoot} from 'react-dom/client';
import {App} from './App';
import {PreparationStartup} from './PreparationStartup';
createRoot(document.getElementById('root')!).render(<PreparationStartup><App/></PreparationStartup>);
