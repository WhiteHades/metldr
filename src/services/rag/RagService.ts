import { vectorStore } from './VectorStore'
import { embeddingProvider } from './EmbeddingProvider'
import { chunkingService } from './ChunkingService'
import type { VectorEntry, SearchResult } from '../../types'

interface ChunkMetadata {
  sourceId: string
  sourceUrl: string
  sourceType: 'article' | 'email' | 'pdf'
  title?: string
}

const EMBEDDING_CONCURRENCY = 4

// simple SimHash for near-duplicate detection
function simHash(text: string): bigint {
  const tokens = text.toLowerCase().split(/\s+/).filter(t => t.length > 2)
  const v = new Int32Array(64).fill(0)
  
  for (const token of tokens) {
    let h = 0n
    for (let i = 0; i < token.length; i++) {
      h = ((h << 5n) - h + BigInt(token.charCodeAt(i))) & 0xFFFFFFFFFFFFFFFFn
    }
    for (let i = 0; i < 64; i++) {
      v[i] += (h & (1n << BigInt(i))) ? 1 : -1
    }
  }
  
  let fingerprint = 0n
  for (let i = 0; i < 64; i++) {
    if (v[i] > 0) fingerprint |= (1n << BigInt(i))
  }
  return fingerprint
}

function hammingDistance(a: bigint, b: bigint): number {
  let xor = a ^ b
  let count = 0
  while (xor) {
    count += Number(xor & 1n)
    xor >>= 1n
  }
  return count
}

export class RagService {
  private contentHashes = new Map<string, string>()
  private simHashes = new Map<string, bigint>()
  private readonly SIMHASH_THRESHOLD = 5

  async index(entry: VectorEntry): Promise<void> {
    try {
      console.log(`[RagService] Indexing ${entry.id}...`)
      const embedding = await embeddingProvider.embedDocument(entry.content)
      await vectorStore.add(entry, embedding)
      console.log(`[RagService] Indexed ${entry.id}`)
    } catch (err) {
      console.error('[RagService] Indexing failed:', err)
    }
  }

  async indexChunks(text: string, metadata: ChunkMetadata): Promise<void> {
    try {
      // check if content unchanged
      const contentHash = this.hashContent(text)
      const existingHash = this.contentHashes.get(metadata.sourceId)
      if (existingHash === contentHash) {
        console.log(`[RagService] Content unchanged for ${metadata.sourceId}, skipping`)
        return
      }

      // check for near-duplicate via SimHash
      const textSimHash = simHash(text)
      for (const [existingId, existingSimHash] of this.simHashes) {
        if (existingId !== metadata.sourceId && hammingDistance(textSimHash, existingSimHash) < this.SIMHASH_THRESHOLD) {
          console.log(`[RagService] Near-duplicate of ${existingId}, skipping`)
          return
        }
      }

      const chunks = await chunkingService.chunkForEmbedding(text)
      if (chunks.length === 0) {
        console.log('[RagService] No chunks to index')
        return
      }
      
      console.log(`[RagService] Indexing ${chunks.length} chunks for ${metadata.sourceId} (parallel=${EMBEDDING_CONCURRENCY})`)
      
      const entries: VectorEntry[] = chunks.map(chunk => ({
        id: `${metadata.sourceType}:${metadata.sourceId}:chunk:${chunk.index}`,
        type: metadata.sourceType,
        content: chunk.text,
        metadata: {
          sourceId: metadata.sourceId,
          sourceUrl: metadata.sourceUrl,
          title: metadata.title || '',
          chunkIndex: chunk.index,
          totalChunks: chunks.length
        },
        timestamp: Date.now()
      }))

      await this.embedAndStoreBatched(entries, EMBEDDING_CONCURRENCY)
      
      // store hashes for future dedup
      this.contentHashes.set(metadata.sourceId, contentHash)
      this.simHashes.set(metadata.sourceId, textSimHash)
      
      console.log(`[RagService] Indexed ${chunks.length} chunks for ${metadata.sourceId}`)
    } catch (err) {
      console.error('[RagService] Chunk indexing failed:', err)
    }
  }

  private hashContent(text: string): string {
    // simple hash: length + first 200 chars + last 200 chars
    const first = text.slice(0, 200)
    const last = text.slice(-200)
    return `${text.length}:${first}:${last}`
  }

  private async embedAndStoreBatched(entries: VectorEntry[], concurrency: number): Promise<void> {
    for (let i = 0; i < entries.length; i += concurrency) {
      const batch = entries.slice(i, i + concurrency)
      const embeddings = await Promise.all(
        batch.map(e => embeddingProvider.embedDocument(e.content))
      )
      await Promise.all(
        batch.map((entry, idx) => vectorStore.add(entry, embeddings[idx]))
      )
    }
  }

  async search(query: string, limit = 5): Promise<SearchResult[]> {
    try {
      const queryEmbedding = await embeddingProvider.embedQuery(query)
      const vectorResults = await vectorStore.search(Array.from(queryEmbedding), limit)
      const keywordResults = await vectorStore.searchKeyword(query, limit)
      const fused = this.fuseResults(vectorResults, keywordResults)
      
      return fused.slice(0, limit)
    } catch (err) {
      console.error('[RagService] Search failed:', err)
      return []
    }
  }

  async searchWithContext(query: string, limit = 3): Promise<string> {
    try {
      const results = await this.search(query, limit)
      if (results.length === 0) return ''
      
      const contextParts = results.map((r, i) => {
        const meta = r.entry.metadata || {}
        const source = meta.title || meta.sourceUrl || r.entry.id
        return `[Source ${i + 1}: ${source}]\n${r.entry.content}`
      })
      
      return contextParts.join('\n\n---\n\n')
    } catch (err) {
      console.error('[RagService] searchWithContext failed:', err)
      return ''
    }
  }

  private fuseResults(vec: SearchResult[], key: SearchResult[]): SearchResult[] {
    const map = new Map<string, SearchResult>()
    vec.forEach(r => map.set(r.entry.id, r))
    key.forEach(r => {
      if (map.has(r.entry.id)) {
        const existing = map.get(r.entry.id)!
        existing.score += 0.2
        existing.matchType = 'hybrid'
      } else {
        map.set(r.entry.id, r)
      }
    })
    return Array.from(map.values()).sort((a, b) => b.score - a.score)
  }
}

export const ragService = new RagService()
