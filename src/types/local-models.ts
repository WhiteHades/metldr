// local model types for transformers.js integration
// 3 core models bundled: embed, classify, ner

export type LocalTask = 'embed' | 'classify' | 'ner' | 'generate'

export interface ModelConfig {
  id: string              // huggingface model id
  task: string            // transformers.js pipeline task
  dtype: 'q8' | 'q4' | 'fp16' | 'fp32'
  device: 'wasm' | 'webgpu'
  sizeBytes: number       // approximate download size
  maxTokens?: number      // context window
  dims?: number           // embedding dimensions
}

// single source of truth for all models
// only primary models - fallback downloads if primary fails
export const MODEL_REGISTRY: Record<LocalTask, ModelConfig> = {
  embed: {
    id: 'nomic-ai/nomic-embed-text-v1.5',
    task: 'feature-extraction',
    dtype: 'q8',
    device: 'wasm',  // wasm for CSP compatibility (webgpu imports from CDN)
    sizeBytes: 68_000_000,
    maxTokens: 8192,
    dims: 768  // matryoshka: slice to 256 for efficiency
  },
  classify: {
    id: 'MoritzLaurer/deberta-v3-xsmall-zeroshot-v1.1-all-33',
    task: 'zero-shot-classification',
    dtype: 'q8',
    device: 'wasm',  // wasm for CSP compatibility
    sizeBytes: 40_000_000
  },
  ner: {
    id: 'Xenova/bert-base-NER',
    task: 'token-classification',
    dtype: 'q8',
    device: 'wasm',  // ner has webgpu issues
    sizeBytes: 55_000_000
  },

  generate: {
    id: 'Xenova/flan-t5-small',
    task: 'text2text-generation',
    dtype: 'q8',
    device: 'wasm',
    sizeBytes: 30_000_000
  }
} as const

// request/response types

export interface EmbedRequest {
  texts: string[]
  isQuery?: boolean
}

export interface EmbedResponse {
  embeddings: number[][]
  dims: number
}

export interface ClassifyRequest {
  text: string
  labels: string[]
  multiLabel?: boolean
}

export interface ClassifyResponse {
  sequence: string
  labels: string[]
  scores: number[]
}

export interface NERRequest {
  text: string
}

export interface NEREntity {
  word: string
  entity: string
  score: number
  start: number
  end: number
}

export interface NERResponse {
  entities: NEREntity[]
}



export interface GenerateRequest {
  prompt: string
  maxTokens?: number
}

export interface GenerateResponse {
  text: string
}

export interface PipelineStatus {
  task: LocalTask
  loaded: boolean
  loading: boolean
  loadTimeMs?: number
  error?: string
}

// discriminated union for sandbox messages
export type OffscreenRequest =
  | { type: 'EMBED'; payload: EmbedRequest }
  | { type: 'CLASSIFY'; payload: ClassifyRequest }
  | { type: 'NER'; payload: NERRequest }

  | { type: 'GENERATE'; payload: GenerateRequest }
  | { type: 'TOKENIZE'; texts: string[] }
  | { type: 'PRELOAD'; tasks: LocalTask[] }
  | { type: 'UNLOAD'; tasks: LocalTask[] }
  | { type: 'STATUS' }
  | { type: 'VOY'; action: 'ADD' | 'SEARCH' | 'SERIALIZE' | 'LOAD'; payload: unknown }

export type OffscreenResponse<T = unknown> = 
  | { ok: true; data: T; timing: number }
  | { ok: false; error: string }
