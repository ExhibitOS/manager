// SPDX-License-Identifier: Apache-2.0
import {defineConfig} from '@playwright/test';
export default defineConfig({testDir:'./src/browser',fullyParallel:false,workers:1,use:{baseURL:'http://127.0.0.1:1420'},webServer:{command:'npm run dev',url:'http://127.0.0.1:1420',reuseExistingServer:false},projects:[{name:'desktop',use:{viewport:{width:1120,height:900}}},{name:'narrow',use:{viewport:{width:640,height:850}}}]});
