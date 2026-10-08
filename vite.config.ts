// SPDX-License-Identifier: Apache-2.0
import {defineConfig} from 'vitest/config';
import react from '@vitejs/plugin-react';
export default defineConfig({plugins:[react()],clearScreen:false,server:{host:'127.0.0.1',port:1420,strictPort:true},build:{target:'es2023'},test:{include:['src/**/*.test.ts']}});
