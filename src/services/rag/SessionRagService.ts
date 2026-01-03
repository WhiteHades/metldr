// session-scoped RAG - in-memory, per-document, ephemeral
// indexes current document for chat without truncation

import { chunkingService } from './ChunkingService'
import { embeddingProvider } from './EmbeddingProvider'
import { localModels } from '../ai/LocalModelProvider'

interface SessionChunk {
  id: string
  text: string
  index: number
  embedding: number[]
}

// simple in-memory inverted index for keyword search
class SessionInvertedIndex {
  private index = new Map<string, Set<number>>()
  private docs = new Map<number, string>()

  add(chunkIndex: number, text: string) {
    this.docs.set(chunkIndex, text)
    for (const token of this.tokenize(text)) {
      if (!this.index.has(token)) this.index.set(token, new Set())
      this.index.get(token)!.add(chunkIndex)
    }
  }

  search(query: string, limit: number): Array<{ index: number; score: number }> {
    const tokens = this.tokenize(query)
    if (tokens.length === 0) return []
    
    const scores = new Map<number, number>()
    for (const token of tokens) {
      const indices = this.index.get(token)
      if (indices) {
        const idf = Math.log(this.docs.size / indices.size + 1)
        for (const idx of indices) {
          scores.set(idx, (scores.get(idx) || 0) + idf)
        }
      }
    }
    
    return Array.from(scores.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, limit)
      .map(([index, score]) => ({ index, score: score / tokens.length }))
  }

  clear() {
    this.index.clear()
    this.docs.clear()
  }

  private tokenize(text: string): string[] {
    return text.toLowerCase()
      .replace(/[^\w\s]/g, ' ')
      .split(/\s+/)
      .filter(t => t.length > 2)
  }
}

class SessionRagService {
  private chunks: SessionChunk[] = []
  private keywordIndex = new SessionInvertedIndex()
  private indexed = false
  private indexingPromise: Promise<void> | null = null
  private contentHash = ''
  
  // quick hash to detect content changes
  private hash(text: string): string {
    return `${text.length}:${text.slice(0, 100)}:${text.slice(-100)}`
  }

  // index document - call once per session/document
  async indexDocument(content: string): Promise<void> {
    if (!content || content.trim().length < 50) return
    
    const newHash = this.hash(content)
    if (this.indexed && this.contentHash === newHash) {
      console.log('[SessionRag] Content unchanged, skipping re-index')
      return
    }

    // if already indexing, wait for it
    if (this.indexingPromise) {
      await this.indexingPromise
      if (this.contentHash === newHash) return
    }

    this.indexingPromise = this._doIndex(content, newHash)
    await this.indexingPromise
  }

  private async _doIndex(content: string, hash: string): Promise<void> {
    const start = performance.now()
    console.log('[SessionRag] Indexing document...', content.length, 'chars')
    
    this.clear()
    
    // chunk content
    const chunkResults = await chunkingService.chunkForEmbedding(content)
    if (chunkResults.length === 0) {
      console.log('[SessionRag] No chunks generated')
      return
    }
    
    console.log('[SessionRag] Generated', chunkResults.length, 'chunks, embedding...')
    
    // embed in batches of 8 for speed (GPU parallelism)
    const BATCH_SIZE = 8
    for (let i = 0; i < chunkResults.length; i += BATCH_SIZE) {
      const batch = chunkResults.slice(i, i + BATCH_SIZE)
      const texts = batch.map(c => c.text)
      
      try {
        const embeddings = await embeddingProvider.embedBatch(texts, false)
        
        for (let j = 0; j < batch.length; j++) {
          const chunk = batch[j]
          this.chunks.push({
            id: `chunk:${chunk.index}`,
            text: chunk.text,
            index: chunk.index,
            embedding: Array.from(embeddings[j])
          })
          this.keywordIndex.add(chunk.index, chunk.text)
        }
      } catch (err) {
        console.error('[SessionRag] Embedding batch failed:', err)
      }
    }
    
    this.indexed = true
    this.contentHash = hash
    this.indexingPromise = null
    
    const elapsed = Math.round(performance.now() - start)
    console.log('[SessionRag] Indexed', this.chunks.length, 'chunks in', elapsed, 'ms')
  }

  // search for relevant chunks
  async search(query: string, limit = 5): Promise<Array<{ text: string; score: number }>> {
    if (!this.indexed || this.chunks.length === 0) {
      return []
    }
    
    try {
      // embed query
      const queryEmbedding = await embeddingProvider.embedQuery(query)
      const queryVec = Array.from(queryEmbedding)
      
      // vector similarity search
      const vectorScores = this.chunks.map(chunk => ({
        index: chunk.index,
        score: this.cosineSimilarity(queryVec, chunk.embedding)
      }))
      
      // keyword search
      const keywordScores = this.keywordIndex.search(query, limit * 2)
      
      // fuse results (RRF - reciprocal rank fusion)
      const fused = this.fuseResults(vectorScores, keywordScores)
      
      // return top K chunks
      return fused
        .slice(0, limit)
        .map(({ index, score }) => ({
          text: this.chunks.find(c => c.index === index)?.text || '',
          score
        }))
        .filter(r => r.text.length > 0)
        
    } catch (err) {
      console.error('[SessionRag] Search failed:', err)
      return []
    }
  }

  // format search results as context for LLM
  async searchForContext(query: string, limit = 5): Promise<string> {
    const results = await this.search(query, limit)
    if (results.length === 0) return ''
    
    return results
      .map((r, i) => `[Section ${i + 1}]\n${r.text}`)
      .join('\n\n---\n\n')
  }

  private cosineSimilarity(a: number[], b: number[]): number {
    let dot = 0, normA = 0, normB = 0
    for (let i = 0; i < a.length; i++) {
      dot += a[i] * b[i]
      normA += a[i] * a[i]
      normB += b[i] * b[i]
    }
    return dot / (Math.sqrt(normA) * Math.sqrt(normB) + 1e-8)
  }

  private fuseResults(
    vec: Array<{ index: number; score: number }>,
    key: Array<{ index: number; score: number }>
  ): Array<{ index: number; score: number }> {
    // normalize vector scores to 0-1
    const maxVec = Math.max(...vec.map(v => v.score), 0.001)
    const vecNorm = vec.map(v => ({ index: v.index, score: v.score / maxVec }))
    
    // combine with weighted average (vector 0.7, keyword 0.3)
    const combined = new Map<number, number>()
    for (const { index, score } of vecNorm) {
      combined.set(index, (combined.get(index) || 0) + score * 0.7)
    }
    for (const { index, score } of key) {
      combined.set(index, (combined.get(index) || 0) + score * 0.3)
    }
    
    return Array.from(combined.entries())
      .map(([index, score]) => ({ index, score }))
      .sort((a, b) => b.score - a.score)
  }

  isIndexed(): boolean {
    return this.indexed
  }

  getChunkCount(): number {
    return this.chunks.length
  }

  clear() {
    this.chunks = []
    this.keywordIndex.clear()
    this.indexed = false
    this.contentHash = ''
    this.indexingPromise = null
  }
}

export const sessionRag = new SessionRagService()
