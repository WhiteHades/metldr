// LocalModelProvider - service worker facade for GPU sandbox
// communicates with sandbox iframe via postMessage

import type { LocalTask } from '../../types/local-models'

// LRU cache for results
class LRUCache<K, V> {
  private cache = new Map<K, V>()
  constructor(private maxSize: number) {}

  get(key: K): V | undefined {
    const value = this.cache.get(key)
    if (value !== undefined) {
      // move to end (most recently used)
      this.cache.delete(key)
      this.cache.set(key, value)
    }
    return value
  }

  set(key: K, value: V): void {
    if (this.cache.has(key)) {
      this.cache.delete(key)
    } else if (this.cache.size >= this.maxSize) {
      // evict oldest
      const oldest = this.cache.keys().next().value
      if (oldest !== undefined) this.cache.delete(oldest)
    }
    this.cache.set(key, value)
  }

  clear(): void {
    this.cache.clear()
  }
}

class LocalModelProvider {
  private static instance: LocalModelProvider
  
  private iframe: HTMLIFrameElement | null = null
  private ready = false
  private readyPromise: Promise<void> | null = null
  private pendingRequests = new Map<string, { resolve: (v: any) => void; reject: (e: Error) => void }>()
  private requestId = 0
  
  private backend: 'webgpu' | 'wasm' = 'wasm'
  
  // sandbox lifecycle tracking - unique ID per sandbox instance
  private sandboxId: string = ''
  private sandboxCreatedAt: number = 0
  
  // caches
  private embedCache = new LRUCache<string, number[]>(200)


  private constructor() {
    // listen for responses
    if (typeof window !== 'undefined') {
      window.addEventListener('message', this.handleResponse.bind(this))
    }
  }

  static getInstance(): LocalModelProvider {
    return LocalModelProvider.instance ??= new LocalModelProvider()
  }

  // check if we're in a context that can create DOM elements
  private isPageContext(): boolean {
    return typeof document !== 'undefined' && typeof window !== 'undefined'
  }

  // create sandbox iframe in current page OR offscreen document
  private async ensureSandbox(): Promise<void> {
    if (this.ready) return
    if (this.readyPromise) return this.readyPromise
    
    this.readyPromise = new Promise(async (resolve, reject) => {
      try {
        if (this.isPageContext()) {
          // PAGE CONTEXT: Use iframe
          
          // check if sandbox already exists (from previous init)
          const existing = document.getElementById('metldr-gpu-sandbox') as HTMLIFrameElement
          if (existing) {
            this.iframe = existing
            this.ready = true
            // generate sandbox ID if not already set (shouldn't happen but be safe)
            if (!this.sandboxId) {
              this.sandboxId = `sandbox_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
              this.sandboxCreatedAt = Date.now()
            }
            console.log('[LocalModels] Reusing existing sandbox, id:', this.sandboxId)
            resolve()
            return
          }
          
          // generate new sandbox ID
          this.sandboxId = `sandbox_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
          this.sandboxCreatedAt = Date.now()
          console.log('[LocalModels] Creating new sandbox, id:', this.sandboxId)
          
          // create hidden iframe
          this.iframe = document.createElement('iframe')
          this.iframe.src = chrome.runtime.getURL('sandbox.html')
          this.iframe.style.display = 'none'
          this.iframe.id = 'metldr-gpu-sandbox'
          
          // wait for ready signal
          const readyHandler = (event: MessageEvent) => {
            if (event.data?._gpuBridgeReady) {
              window.removeEventListener('message', readyHandler)
              this.ready = true
              console.log('[LocalModels] Sandbox ready')
              
              // init backend
              this.send({ type: 'INIT' }).then(res => {
                if (res.ok) this.backend = res.data.backend
              })
              
              resolve()
            }
          }
          window.addEventListener('message', readyHandler)
          
          document.body.appendChild(this.iframe)
          
          // timeout
          setTimeout(() => {
            if (!this.ready) {
              reject(new Error('Sandbox initialization timeout'))
            }
          }, 10000)
          
        } else {
          // SERVICE WORKER CONTEXT: Use Offscreen API
          
          if (!chrome.offscreen) {
            reject(new Error('LocalModelProvider: Not in page context and chrome.offscreen not available'))
            return
          }

          const hasDoc = await chrome.offscreen.hasDocument()
          if (!hasDoc) {
            await chrome.offscreen.createDocument({
              url: 'offscreen.html',
              reasons: [chrome.offscreen.Reason.DOM_SCRAPING],
              justification: 'Run local AI models for RAG and inference'
            })
          }

          // Wait for bridge to be ready via PING
          let attempts = 0
          while (attempts < 20) {
            try {
              const pong = await chrome.runtime.sendMessage({ target: 'offscreen', type: 'PING' })
              if (pong?.status === 'pong') {
                this.ready = true
                // generate sandbox ID for offscreen
                if (!this.sandboxId) {
                  this.sandboxId = `offscreen_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
                  this.sandboxCreatedAt = Date.now()
                }
                console.log('[LocalModels] Offscreen sandbox ready, id:', this.sandboxId)
                
                // init backend
                this.send({ type: 'INIT' }).then(res => {
                  if (res.ok) this.backend = res.data.backend
                })
                
                resolve()
                return
              }
            } catch (e) {
              // ignore connection errors while booting
            }
            await new Promise(r => setTimeout(r, 500))
            attempts++
          }
          reject(new Error('Offscreen sandbox initialization timeout'))
        }
      } catch (err) {
        reject(err)
      }
    })
    
    return this.readyPromise
  }

  // handle responses from sandbox
  private handleResponse(event: MessageEvent): void {
    if (!event.data?._gpuBridgeResponse) return
    
    const { requestId, ...response } = event.data
    const pending = this.pendingRequests.get(requestId)
    if (pending) {
      this.pendingRequests.delete(requestId)
      pending.resolve(response)
    }
  }

  // send message to sandbox
  private async send(message: any): Promise<any> {
    await this.ensureSandbox()
    
    const id = `req_${++this.requestId}`
    
    if (this.isPageContext()) {
      return new Promise((resolve, reject) => {
        this.pendingRequests.set(id, { resolve, reject })
        
        this.iframe?.contentWindow?.postMessage({
          ...message,
          _gpuBridge: true,
          requestId: id
        }, '*')
        
        setTimeout(() => {
          if (this.pendingRequests.has(id)) {
            this.pendingRequests.delete(id)
            reject(new Error(`Request ${message.type} timed out`))
          }
        }, 180000)
      })
    } else {
      // Offscreen communication with RETRY logic
      try {
        const response = await chrome.runtime.sendMessage({
          target: 'offscreen',
          type: message.type,
          data: {
            ...message,
            _gpuBridge: true,
            requestId: id
          }
        })
        return response
      } catch (err) {
        console.warn('[LocalModels] Offscreen request failed, retrying in 1s...', err)
        
        // Force re-initialization
        this.ready = false
        this.readyPromise = null
        
        // Wait a bit
        await new Promise(r => setTimeout(r, 1000))
        
        // Retry ONCE
        await this.ensureSandbox()
        try {
           const response = await chrome.runtime.sendMessage({
            target: 'offscreen',
            type: message.type,
            data: {
              ...message,
              _gpuBridge: true,
              requestId: id
            }
          })
          return response
        } catch (retryErr) {
           throw new Error(`Offscreen request failed after retry: ${(retryErr as Error).message}`)
        }
      }
    }
  }

  // public API

  async embed(text: string, isQuery = false): Promise<number[]> {
    const cacheKey = `${isQuery}:${text.slice(0, 100)}`
    const cached = this.embedCache.get(cacheKey)
    if (cached) return cached
    
    const res = await this.send({ type: 'EMBED', payload: { texts: [text], isQuery } })
    if (!res.ok) throw new Error(res.error)
    
    const embedding = res.data.embeddings[0]
    this.embedCache.set(cacheKey, embedding)
    return embedding
  }

  async embedBatch(texts: string[], isQuery = false): Promise<number[][]> {
    const res = await this.send({ type: 'EMBED', payload: { texts, isQuery } })
    if (!res.ok) throw new Error(res.error)
    return res.data.embeddings
  }

  async tokenize(texts: string[]): Promise<number[]> {
    const res = await this.send({ type: 'TOKENIZE', texts })
    if (!res.ok) throw new Error(res.error)
    return res.data.counts
  }

  async preload(tasks: LocalTask[]): Promise<void> {
    const res = await this.send({ type: 'PRELOAD', tasks })
    if (!res.ok) throw new Error(res.error)
  }

  async getStatus(): Promise<{ tasks: any[]; backend: string; poolSize: number }> {
    const res = await this.send({ type: 'STATUS' })
    if (!res.ok) throw new Error(res.error)
    return res.data
  }

  getBackend(): string {
    return this.backend
  }

  clearCaches(): void {
    this.embedCache.clear()
  }

  // VOY vector store operations

  async voyAdd(id: string, embedding: number[], title?: string, url?: string): Promise<void> {
    const res = await this.send({ type: 'VOY_ADD', id, embedding, title: title || id, url: url || '' })
    if (!res.ok) throw new Error(res.error)
  }

  async voySearch(embedding: number[], limit = 5): Promise<Array<{ id: string; score: number }>> {
    const res = await this.send({ type: 'VOY_SEARCH', embedding, limit })
    if (!res.ok) throw new Error(res.error)
    return res.data.results
  }

  async voySerialize(): Promise<Uint8Array> {
    const res = await this.send({ type: 'VOY_SERIALIZE' })
    if (!res.ok) throw new Error(res.error)
    return res.data.serialized
  }

  async voyLoad(data: Uint8Array): Promise<void> {
    const res = await this.send({ type: 'VOY_LOAD', data })
    if (!res.ok) throw new Error(res.error)
  }

  // sandbox lifecycle tracking
  getSandboxId(): string {
    return this.sandboxId
  }

  getSandboxCreatedAt(): number {
    return this.sandboxCreatedAt
  }

  isReady(): boolean {
    return this.ready
  }
}

export const localModels = LocalModelProvider.getInstance()
