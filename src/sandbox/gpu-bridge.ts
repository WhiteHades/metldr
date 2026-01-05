// gpu-bridge.ts - runs inside sandbox iframe (CSP-exempt for WebGPU)
// handles all transformers.js operations via postMessage API

import { pipeline, env, AutoTokenizer, type Pipeline } from '@huggingface/transformers'
import type { LocalTask, OffscreenRequest, OffscreenResponse } from '../types/local-models'

// voy-search WASM binding (loaded manually to avoid bundler-style imports)
let Voy: any = null
let voyIndex: any = null
let voyInitPromise: Promise<void> | null = null

async function ensureVoy() {
  if (Voy) return voyIndex
  if (voyInitPromise) {
    await voyInitPromise
    return voyIndex
  }
  
  voyInitPromise = (async () => {
    try {
      // dynamically import the JS bindings (bundled by esbuild)
      const voyBindings = await import('./voy-bindings')
      
      // fetch the WASM binary from assets
      const wasmUrl = './assets/voy_search_bg.wasm'
      const wasmResponse = await fetch(wasmUrl)
      
      // instantiate WASM with the import object from bindings
      const wasmModule = await WebAssembly.instantiateStreaming(wasmResponse, {
        './voy_search_bg.js': voyBindings
      })
      
      // connect the WASM module to the JS bindings
      voyBindings.__wbg_set_wasm(wasmModule.instance.exports)
      
      // get the Voy class
      Voy = voyBindings.Voy
      voyIndex = new Voy({ embeddings: [] })
      console.log('[GPU Bridge] Voy WASM initialized successfully')
    } catch (err) {
      console.error('[GPU Bridge] Voy WASM init failed:', err)
      throw err
    }
  })()
  
  await voyInitPromise
  return voyIndex
}

// model registry - embedding only
const MODEL_REGISTRY = {
  embed: {
    id: 'nomic-ai/nomic-embed-text-v1.5',
    task: 'feature-extraction',
    dtype: 'q8',
    priority: 'bundled'
  }
} as const

// local model paths for bundled models (embedding only)
const LOCAL_MODEL_PATHS: Record<string, string> = {
  'nomic-ai/nomic-embed-text-v1.5': 'models/nomic-embed-text-v1.5/'
}

// configure transformers.js
env.allowLocalModels = true // load bundled models from extension files
env.useBrowserCache = false // disabled - all models bundled locally (sandbox can't access cache API anyway)
env.allowRemoteModels = false // disabled - all models are bundled, no remote fetching needed
// set local paths for bundled models
env.localModelPath = '' // base path (extension root)

// configure ONNX runtime to use bundled WASM (critical for Chrome Store CSP compliance)
// @ts-ignore - accessing internal ONNX config
if (env.backends?.onnx?.wasm) {
  // @ts-ignore
  env.backends.onnx.wasm.wasmPaths = './assets/'
}

// state
let currentBackend: 'webgpu' | 'wasm' = 'wasm'
const pipelines = new Map<LocalTask, Pipeline>()
const loading = new Map<LocalTask, Promise<Pipeline>>()
let tokenizer: any = null
const MAX_LOADED = 3

// init backend with webgpu → wasm fallback
async function initBackend(): Promise<'webgpu' | 'wasm'> {
  try {
    // test webgpu availability
    const gpu = (navigator as any).gpu
    if (!gpu) throw new Error('WebGPU not available')
    const adapter = await gpu.requestAdapter()
    if (!adapter) throw new Error('No GPU adapter found')
    
    console.log('[GPU Bridge] WebGPU available, using GPU backend')
    currentBackend = 'webgpu'
    return 'webgpu'
  } catch (err) {
    console.warn('[GPU Bridge] WebGPU unavailable, using WASM:', (err as Error).message)
    currentBackend = 'wasm'
    return 'wasm'
  }
}

// get or load pipeline with deduplication and LRU eviction
async function getPipeline(task: LocalTask): Promise<Pipeline> {
  const cached = pipelines.get(task)
  if (cached) return cached
  
  const inflight = loading.get(task)
  if (inflight) return inflight
  
  // evict LRU if at capacity
  if (pipelines.size >= MAX_LOADED) {
    const oldest = pipelines.keys().next().value
    if (oldest) {
      console.log(`[GPU Bridge] Evicting ${oldest}`)
      pipelines.delete(oldest)
    }
  }
  
  const loadPromise = loadPipeline(task)
  loading.set(task, loadPromise)
  
  try {
    return await loadPromise
  } finally {
    loading.delete(task)
  }
}

async function loadPipeline(task: LocalTask): Promise<Pipeline> {
  const config = MODEL_REGISTRY[task]
  const start = performance.now()
  
  console.log(`[GPU Bridge] Loading ${task}: ${config.id} on ${currentBackend}`)
  
  // use local path if model is bundled, otherwise use model ID
  const modelPath = LOCAL_MODEL_PATHS[config.id] || config.id
  
  try {
    const p = await pipeline(config.task as any, modelPath, {
      dtype: config.dtype as any,
      device: currentBackend
    })
    
    const elapsed = performance.now() - start
    pipelines.set(task, p)
    console.log(`[GPU Bridge] Loaded ${task} in ${elapsed.toFixed(0)}ms`)
    return p
  } catch (err) {
    // if webgpu fails, try wasm fallback
    if (currentBackend === 'webgpu') {
      console.warn(`[GPU Bridge] WebGPU failed for ${task}, retrying with WASM`)
      currentBackend = 'wasm'
      return loadPipeline(task)
    }
    throw err
  }
}

// embedding with matryoshka slicing
// nomic-embed-text-v1.5 has 8192 token limit (~4 chars/token)
const MAX_EMBED_CHARS = 24000 // ~6000 tokens, leaving headroom

async function embed(texts: string[], isQuery: boolean): Promise<number[][]> {
  console.log('[GPU Bridge] embed called:', { textCount: texts.length, isQuery })
  
  const extractor = await getPipeline('embed')
  if (!extractor) {
    throw new Error('Failed to get embedding pipeline')
  }
  
  const results: number[][] = []
  const targetDims = 256 // matryoshka slice
  
  for (let i = 0; i < texts.length; i++) {
    const text = texts[i]
    
    // warn about oversized text (chunking should prevent this)
    if (text.length > MAX_EMBED_CHARS) {
      console.warn(`[GPU Bridge] WARNING: text ${i + 1} is ${text.length} chars (>${MAX_EMBED_CHARS}), may exceed token limit`)
    }
    
    const prefixed = isQuery ? `search_query: ${text}` : `search_document: ${text}`
    
    console.log(`[GPU Bridge] Embedding text ${i + 1}/${texts.length} (${text.length} chars)`)
    const output = await extractor(prefixed, { pooling: 'mean', normalize: true })
    
    if (!output || !output.data) {
      throw new Error(`Embedding failed for text ${i + 1}: no output data`)
    }
    
    const full = output.data as Float32Array
    const sliced = full.slice(0, targetDims)
    
    // renormalize after slice
    let norm = 0
    for (let j = 0; j < sliced.length; j++) norm += sliced[j] ** 2
    norm = Math.sqrt(norm)
    if (norm > 0) for (let j = 0; j < sliced.length; j++) sliced[j] /= norm
    
    results.push(Array.from(sliced))
  }
  
  console.log('[GPU Bridge] embed complete:', { resultCount: results.length })
  return results
}

// tokenizer
async function getTokenizer() {
  if (tokenizer) return tokenizer
  const config = MODEL_REGISTRY.embed
  // use local path if model is bundled
  const modelPath = LOCAL_MODEL_PATHS[config.id] || config.id
  tokenizer = await AutoTokenizer.from_pretrained(modelPath)
  return tokenizer
}

async function tokenize(texts: string[]): Promise<number[]> {
  const tok = await getTokenizer()
  const results: number[] = []
  for (const text of texts) {
    const { input_ids } = await tok(text)
    results.push(input_ids.size)
  }
  return results
}

// message handler
async function handleMessage(request: any): Promise<any> {
  const start = performance.now()
  
  try {
    let data: any
    
    switch (request.type) {
      case 'INIT':
        const backend = await initBackend()
        data = { backend }
        break
        
      case 'EMBED':
        data = { embeddings: await embed(request.payload.texts, request.payload.isQuery ?? false), dims: 256 }
        break
        
      case 'TOKENIZE':
        data = { counts: await tokenize(request.texts) }
        break
        
      case 'PRELOAD':
        await Promise.all((request.tasks as LocalTask[]).map(t => getPipeline(t)))
        data = { preloaded: request.tasks }
        break
        
      case 'STATUS':
        const tasks = (Object.keys(MODEL_REGISTRY) as LocalTask[]).map(task => ({
          task,
          loaded: pipelines.has(task)
        }))
        data = { tasks, backend: currentBackend, poolSize: pipelines.size }
        break
        
      // VOY vector store operations
      case 'VOY_ADD':
        await ensureVoy()
        // voy-search requires: id, title, url, embeddings
        voyIndex.add({ embeddings: [{ 
          id: request.id, 
          title: request.title || request.id, 
          url: request.url || '',
          embeddings: request.embedding 
        }] })
        data = { added: request.id }
        break
        
      case 'VOY_SEARCH':
        await ensureVoy()
        const searchResults = voyIndex.search(request.embedding, request.limit || 5)
        data = { results: searchResults.neighbors.map((n: any) => ({ id: n.id, score: n.distance })) }
        break
        
      case 'VOY_SERIALIZE':
        await ensureVoy()
        data = { serialized: voyIndex.serialize() }
        break
        
      case 'VOY_LOAD':
        await ensureVoy()
        if (Voy && request.data) {
          voyIndex = Voy.deserialize(request.data)
        }
        data = { loaded: true }
        break
        
      default:
        throw new Error(`Unknown message type: ${request.type}`)
    }
    
    return { ok: true, data, timing: performance.now() - start }
  } catch (err) {
    // comprehensive error capture
    let errorMessage = 'Unknown error'
    if (err instanceof Error) {
      errorMessage = err.message || err.name || String(err)
    } else if (typeof err === 'string') {
      errorMessage = err
    } else if (err && typeof err === 'object') {
      errorMessage = JSON.stringify(err)
    }
    console.error('[GPU Bridge] Error:', errorMessage, err)
    return { ok: false, error: errorMessage || 'Sandbox execution failed' }
  }
}

// listen for messages from parent
window.addEventListener('message', async (event) => {
  // only accept messages with our protocol
  if (!event.data || !event.data.type || !event.data._gpuBridge) return
  
  const response = await handleMessage(event.data)
  
  // respond to parent
  window.parent.postMessage({
    _gpuBridgeResponse: true,
    requestId: event.data.requestId,
    ...response
  }, '*')
})

// signal ready
console.log('[GPU Bridge] Sandbox loaded, waiting for messages')
window.parent.postMessage({ _gpuBridgeReady: true }, '*')
