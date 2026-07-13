import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  // `node_modules.<env>` is the per-environment dependency tree the justfile
  // installs and symlinks as `node_modules` (see `just fe-deps`). ESLint's built-in
  // node_modules ignore does not match that name, so it would otherwise lint the
  // whole dependency tree and die on the first package with its own eslint config.
  globalIgnores(['dist', 'node_modules*']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
  },
])
