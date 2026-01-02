// re-export voy-search bindings for manual WASM initialization
// this file gets bundled by esbuild, then we manually init the WASM

// @ts-ignore - voy-search has no type declarations
export * from 'voy-search/voy_search_bg.js'
