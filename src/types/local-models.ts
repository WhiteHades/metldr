// local model types for transformers.js integration
// embedding model bundled only

export type LocalTask = 'embed'

export interface ModelConfig {
  id: string              
  task: string            
  dtype: 'q8' | 'q4' | 'fp16' | 'fp32'
  device: 'wasm' | 'webgpu'
  sizeBytes: number       
  maxTokens?: number      
  dims?: number           
}

export const MODEL_REGISTRY: Record<LocalTask, ModelConfig> = {
  embed: {
    id: 'nomic-ai/nomic-embed-text-v1.5',
    task: 'feature-extraction',
    dtype: 'q8',
    device: 'wasm',  // wasm for CSP compatibility
    sizeBytes: 68_000_000,
    maxTokens: 8192,
    dims: 768  // matryoshka: slice to 256 for efficiency
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
  | { type: 'TOKENIZE'; texts: string[] }
  | { type: 'PRELOAD'; tasks: LocalTask[] }
  | { type: 'UNLOAD'; tasks: LocalTask[] }
  | { type: 'STATUS' }
  | { type: 'VOY'; action: 'ADD' | 'SEARCH' | 'SERIALIZE' | 'LOAD'; payload: unknown }

export type OffscreenResponse<T = unknown> = 
  | { ok: true; data: T; timing: number }
  | { ok: false; error: string }
