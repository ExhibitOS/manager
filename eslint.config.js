// SPDX-License-Identifier: Apache-2.0
import tseslint from 'typescript-eslint';
export default tseslint.config({ignores:['dist/**','src-tauri/**','node_modules/**','test-results/**']},...tseslint.configs.recommended);
