import { localModels } from '../ai/LocalModelProvider'

interface EmbeddingStats {
  attempts: number
  totalMs: number
  lastError?: string
}

export class EmbeddingProvider {
  private readonly maxRetries = 5
  private readonly baseDelayMs = 500
  private lastStats: EmbeddingStats | null = null

  async embed(text: string, isQuery = false): Promise<Float32Array> {
    const startTime = Date.now()
    let lastError: Error | null = null
    let attempts = 0
    
    for (let attempt = 1; attempt <= this.maxRetries; attempt++) {
      attempts = attempt
      try {
        const embedding = await localModels.embed(text, isQuery)
        
        // record successful stats
        this.lastStats = {
          attempts,
          totalMs: Date.now() - startTime
        }
        
        if (attempts > 1) {
          console.log(`[EmbeddingProvider] Success after ${attempts} attempts (${this.lastStats.totalMs}ms total)`)
        }
        
        return new Float32Array(embedding)
      } catch (err) {
        lastError = err as Error
        const wallClockSoFar = Date.now() - startTime
        console.warn(`[EmbeddingProvider] Attempt ${attempt}/${this.maxRetries} failed after ${wallClockSoFar}ms:`, lastError.message)
        
        const isSandboxIssue = lastError.message.includes('timeout') ||
                       lastError.message.includes('Sandbox')
        
        if (isSandboxIssue && attempt < this.maxRetries) {
          const delay = this.baseDelayMs * Math.pow(2, attempt - 1)
          console.log(`[EmbeddingProvider] Sandbox issue, retrying in ${delay}ms (wall clock: ${wallClockSoFar}ms)`)
          await this.sleep(delay)
          continue
        }
        
        if (attempt >= this.maxRetries) {
          this.lastStats = {
            attempts,
            totalMs: Date.now() - startTime,
            lastError: lastError.message
          }
          throw lastError
        }
      }
    }
    
    // should not reach here but just in case
    this.lastStats = {
      attempts,
      totalMs: Date.now() - startTime,
      lastError: lastError?.message || 'Max retries exceeded'
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
    const startTime = Date.now()
    try {
      const embeddings = await localModels.embedBatch(texts, isQuery)
      console.log(`[EmbeddingProvider] Batch of ${texts.length} embedded in ${Date.now() - startTime}ms`)
      return embeddings.map(e => new Float32Array(e))
    } catch (err) {
      console.error(`[EmbeddingProvider] Batch embedding failed after ${Date.now() - startTime}ms:`, err)
      throw err
    }
  }

  private sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
  }

  getLastStats(): EmbeddingStats | null {
    return this.lastStats
  }
}

export const embeddingProvider = new EmbeddingProvider()
