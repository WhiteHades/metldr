import { copyFileSync, mkdirSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const sources = [
  'node_modules/@huggingface/transformers/dist/ort-wasm-simd-threaded.jsep.mjs',
  'node_modules/@huggingface/transformers/dist/ort-wasm-simd-threaded.jsep.wasm',
  'node_modules/voy-search/voy_search_bg.wasm',
];

export function copySandboxAssets(directory) {
  mkdirSync(directory, { recursive: true });
  for (const source of sources) {
    copyFileSync(resolve(source), resolve(directory, basename(source)));
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  copySandboxAssets('dist/assets');
}
