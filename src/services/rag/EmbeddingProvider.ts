// embedding provider - thin wrapper around LocalModelProvider
// maintains backward compatibility while using new infrastructure

import { localModels } from '../ai/LocalModelProvider'

// check if in page context (has document)
function isPageContext(): boolean {
  return typeof document !== 'undefined' && typeof window !== 'undefined'
}

export class EmbeddingProvider {
  private readonly maxRetries = 3
  private readonly baseDelayMs = 100

  async embed(text: string, isQuery = false): Promise<Float32Array> {
    // fail fast in service worker context - sandbox requires page context
    // sandbox now handles service worker context via offscreen document
    // if (!isPageContext()) {
    //   throw new Error('EmbeddingProvider requires page context (side panel). RAG indexing must be triggered from page, not service worker.')
    // }
    
    let lastError: Error | null = null
    
    for (let attempt = 1; attempt <= this.maxRetries; attempt++) {
      try {
        const embedding = await localModels.embed(text, isQuery)
        return new Float32Array(embedding)
      } catch (err) {
        lastError = err as Error
        console.warn(`[EmbeddingProvider] Attempt ${attempt}/${this.maxRetries} failed:`, lastError.message)
        
        const isDead = lastError.message.includes('timeout') ||
                       lastError.message.includes('Sandbox')
        
        if (isDead && attempt < this.maxRetries) {
          console.log('[EmbeddingProvider] Sandbox issue, will retry...')
          await this.sleep(this.baseDelayMs * Math.pow(4, attempt - 1))
          continue
        }
        
        if (attempt >= this.maxRetries) throw lastError
      }
    }
    
    throw lastError || new Error('Max retries exceeded')
  }

  async embedQuery(text: string): Promise<Float32Array> {
    return this.embed(text, true)
  }

  async embedDocument(text: string): Promise<Float32Array> {
    return this.embed(text, false)
  }

  async embedBatch(texts: string[], isQuery = false): Promise<Float32Array[]> {
    const embeddings = await localModels.embedBatch(texts, isQuery)
    return embeddings.map(e => new Float32Array(e))
  }

  private sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
  }
}

export const embeddingProvider = new EmbeddingProvider()
