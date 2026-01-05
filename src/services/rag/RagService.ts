import { vectorStore } from './VectorStore'
import { embeddingProvider } from './EmbeddingProvider'
import { chunkingService } from './ChunkingService'
import { databaseService, DB_CONFIGS } from '../DatabaseService'
import type { VectorEntry, SearchResult } from '../../types'

interface ChunkMetadata {
  sourceId: string
  sourceUrl: string
  sourceType: 'article' | 'email' | 'pdf'
  title?: string
}

interface RagMetadataEntry {
  sourceId: string
  contentHash: string
  chunkCount: number
  timestamp: number
}

interface IndexingStats {
  sourceId: string
  wallClockMs: number
  chunkCount: number
  embeddingMs: number
  storageMs: number
  skipped: boolean
  skipReason?: 'unchanged' | 'duplicate' | 'no_chunks'
}

const EMBEDDING_CONCURRENCY = 4
const STORE_RAG_METADATA = 'rag_metadata'

// simhash: locality-sensitive hashing for near-duplicate detection
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

function bigintToHex(n: bigint): string {
  return n.toString(16).padStart(16, '0')
}

function hexToBigint(hex: string): bigint {
  return BigInt('0x' + hex)
}

export class RagService {
  private contentHashes = new Map<string, string>()
  private metadataLoaded = false
  private loadingPromise: Promise<void> | null = null
  private activeIndexing = new Map<string, Promise<void>>()
  private lastStats: IndexingStats | null = null

  private async ensureMetadataLoaded(): Promise<void> {
    if (this.metadataLoaded) return
    if (this.loadingPromise) return this.loadingPromise

    this.loadingPromise = (async () => {
      const startTime = Date.now()
      try {
        console.log('[RagService] Loading persisted metadata from IDB...')
        const entries = await databaseService.getAll<RagMetadataEntry>(DB_CONFIGS.cache, STORE_RAG_METADATA)
        
        for (const entry of entries) {
          this.contentHashes.set(entry.sourceId, entry.contentHash)
        }
        
        console.log(`[RagService] Loaded ${entries.length} metadata entries (${Date.now() - startTime}ms)`)
        this.metadataLoaded = true
      } catch (err) {
        console.error('[RagService] Failed to load metadata:', err)
        this.metadataLoaded = true // proceed anyway to avoid infinite loops
      }
    })()

    await this.loadingPromise
  }

  private async persistMetadata(sourceId: string, contentHash: string, chunkCount: number): Promise<void> {
    try {
      const entry: RagMetadataEntry = {
        sourceId,
        contentHash,
        chunkCount,
        timestamp: Date.now()
      }
      await databaseService.put(DB_CONFIGS.cache, STORE_RAG_METADATA, entry)
      console.log(`[RagService] Persisted metadata for ${sourceId.slice(0, 50)}`)
    } catch (err) {
      console.error('[RagService] Failed to persist metadata:', err)
    }
  }

  async index(entry: VectorEntry): Promise<void> {
    const startTime = Date.now()
    try {
      console.log(`[RagService] Indexing single entry: ${entry.id}`)
      const embedding = await embeddingProvider.embedDocument(entry.content)
      await vectorStore.add(entry, embedding)
      console.log(`[RagService] Indexed ${entry.id} (${Date.now() - startTime}ms)`)
    } catch (err) {
      console.error('[RagService] Single indexing failed:', err)
    }
  }

  async indexChunks(text: string, metadata: ChunkMetadata): Promise<void> {
    const wallClockStart = Date.now()
    await this.ensureMetadataLoaded()

    // check for concurrent indexing of same source
    const existing = this.activeIndexing.get(metadata.sourceId)
    if (existing) {
      console.log(`[RagService] Already indexing ${metadata.sourceId.slice(0, 50)}, waiting...`)
      await existing
      return
    }

    const indexPromise = this._doIndexChunks(text, metadata, wallClockStart)
    this.activeIndexing.set(metadata.sourceId, indexPromise)
    
    try {
      await indexPromise
    } finally {
      this.activeIndexing.delete(metadata.sourceId)
    }
  }

  private async _doIndexChunks(text: string, metadata: ChunkMetadata, wallClockStart: number): Promise<void> {
    const stats: IndexingStats = {
      sourceId: metadata.sourceId,
      wallClockMs: 0,
      chunkCount: 0,
      embeddingMs: 0,
      storageMs: 0,
      skipped: false
    }

    try {
      const contentHash = this.hashContent(text)
      const existingHash = this.contentHashes.get(metadata.sourceId)
      
      // check if content unchanged (persisted deduplication)
      if (existingHash === contentHash) {
        // verify VOY index actually has this data
        const voyHasData = await this.verifyVoyIndex(metadata.sourceId)
        if (voyHasData) {
          stats.skipped = true
          stats.skipReason = 'unchanged'
          stats.wallClockMs = Date.now() - wallClockStart
          this.lastStats = stats
          console.log(`[RagService] SKIPPED: ${metadata.sourceId.slice(0, 50)} (content unchanged, VOY verified) [${stats.wallClockMs}ms]`)
          return
        } else {
          console.log(`[RagService] Metadata says indexed but VOY empty, re-indexing...`)
          // fall through to re-index
        }
      }

      const contentType = metadata.sourceType === 'email' ? 'email' : metadata.sourceType === 'pdf' ? 'pdf' : 'article'
      const chunks = await chunkingService.chunkForEmbedding(text, contentType as 'email' | 'article' | 'pdf')
      if (chunks.length === 0) {
        stats.skipped = true
        stats.skipReason = 'no_chunks'
        stats.wallClockMs = Date.now() - wallClockStart
        this.lastStats = stats
        console.log(`[RagService] SKIPPED: No chunks to index [${stats.wallClockMs}ms]`)
        return
      }
      
      stats.chunkCount = chunks.length
      console.log(`[RagService] INDEXING: ${chunks.length} chunks for ${metadata.sourceId.slice(0, 50)}`)
      
      // create vector entries
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

      // embed and store with timing
      const embedStart = Date.now()
      await this.embedAndStoreBatched(entries, EMBEDDING_CONCURRENCY)
      stats.embeddingMs = Date.now() - embedStart
      
      const persistStart = Date.now()
      this.contentHashes.set(metadata.sourceId, contentHash)
      await this.persistMetadata(metadata.sourceId, contentHash, chunks.length)
      
      await vectorStore.forceSave()
      stats.storageMs = Date.now() - persistStart
      
      stats.wallClockMs = Date.now() - wallClockStart
      this.lastStats = stats
      
      console.log(`[RagService] INDEXED: ${chunks.length} chunks for ${metadata.sourceId.slice(0, 50)}`)
      console.log(`[RagService] Timing: total=${stats.wallClockMs}ms, embed=${stats.embeddingMs}ms, persist=${stats.storageMs}ms`)
    } catch (err) {
      stats.wallClockMs = Date.now() - wallClockStart
      this.lastStats = stats
      console.error(`[RagService] Chunk indexing FAILED after ${stats.wallClockMs}ms:`, err)
    }
  }

  // verify that VOY index actually has embeddings for this source
  private async verifyVoyIndex(sourceId: string): Promise<boolean> {
    try {
      const docCount = vectorStore.getDocCount()
      if (docCount === 0) {
        console.log('[RagService] VOY verification: index is empty')
        return false
      }
      
      const hasDoc = await vectorStore.hasDocument(sourceId)
      console.log(`[RagService] VOY verification: hasDoc=${hasDoc}, totalDocs=${docCount}`)
      return hasDoc
    } catch (err) {
      console.warn('[RagService] VOY verification error:', err)
      return false
    }
  }

  private hashContent(text: string): string {
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

  async searchWithContext(query: string, limit = 3, sourceUrl?: string): Promise<string> {
    try {
      const results = await this.search(query, limit * 4)
      if (results.length === 0) return ''
      
      let filtered = results
      if (sourceUrl) {
        const sourceBase = sourceUrl.split('?')[0]
        filtered = results.filter(r => {
          const entrySourceUrl = r.entry.metadata?.sourceUrl as string | undefined
          if (!entrySourceUrl || typeof entrySourceUrl !== 'string') return false
          const entryBase = entrySourceUrl.split('?')[0]
          return entrySourceUrl === sourceUrl || 
                 entryBase === sourceBase ||
                 entryBase.startsWith(sourceBase) ||
                 sourceBase.startsWith(entryBase)
        })
        
        console.log(`[RagService] searchWithContext: ${results.length} total -> ${filtered.length} filtered for ${sourceUrl.slice(0, 50)}`)
        
        if (filtered.length === 0) {
          console.log('[RagService] No RAG results for current page, using raw content instead')
          return ''
        }
      }
      
      // sort by score
      filtered.sort((a, b) => b.score - a.score)
      
      const contextParts = filtered.slice(0, limit).map((r, i) => {
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

  async hasIndexedContent(sourceUrl: string): Promise<boolean> {
    await this.ensureMetadataLoaded()
    
    // check metadata first (fast)
    if (this.contentHashes.has(sourceUrl)) {
      // verify VOY actually has the data
      const voyHasData = await this.verifyVoyIndex(sourceUrl)
      if (voyHasData) {
        return true
      }
      console.log(`[RagService] hasIndexedContent: metadata exists but VOY empty for ${sourceUrl.slice(0, 50)}`)
    }

    // fallback: check inverted index
    try {
      const results = await vectorStore.searchKeyword(sourceUrl, 1)
      return results.some(r => r.entry.metadata?.sourceUrl === sourceUrl)
    } catch {
      return false
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

  isIndexing(sourceId: string): boolean {
    return this.activeIndexing.has(sourceId)
  }

  getActiveIndexingCount(): number {
    return this.activeIndexing.size
  }

  getLastStats(): IndexingStats | null {
    return this.lastStats
  }
  
  // get persisted metadata count (for debugging)
  getMetadataCount(): number {
    return this.contentHashes.size
  }
}

export const ragService = new RagService()
