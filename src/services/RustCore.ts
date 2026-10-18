import initCore from '../../crates/metldr-core/pkg/metldr_core'
import coreWasmUrl from '../../crates/metldr-core/pkg/metldr_core_bg.wasm?url'

let rustCoreReady: ReturnType<typeof initCore> | undefined

export function loadRustCore(): ReturnType<typeof initCore> {
  return rustCoreReady ??= initCore({
    module_or_path: chrome.runtime.getURL(coreWasmUrl.replace(/^\//, ''))
  }).catch(error => {
    rustCoreReady = undefined
    throw error
  })
}
