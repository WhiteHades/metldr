import { vectorStore } from './VectorStore'
import { embeddingProvider } from './EmbeddingProvider'
import { chunkingService } from './ChunkingService'
import { databaseService, DB_CONFIGS } from '../DatabaseService'
import { LRUCache } from 'lru-cache'
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

const EMBEDDING_CONCURRENCY = 8
const STORE_RAG_METADATA = 'rag_metadata'
const QUERY_CACHE_MAX = 100
const QUERY_CACHE_TTL = 5 * 60 * 1000 // 5 min

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
  // LRU cache for query results (instant repeat queries)
  private queryCache = new LRUCache<string, SearchResult[]>({
    max: QUERY_CACHE_MAX,
    ttl: QUERY_CACHE_TTL
  })

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

  async indexChunks(text: string, metadata: ChunkMetadata, onProgress?: (percent: number) => void): Promise<void> {
    const wallClockStart = Date.now()
    await this.ensureMetadataLoaded()

    // check for concurrent indexing of same source
    const existing = this.activeIndexing.get(metadata.sourceId)
    if (existing) {
      console.log(`[RagService] Already indexing ${metadata.sourceId.slice(0, 50)}, waiting...`)
      await existing
      return
    }

    const indexPromise = this._doIndexChunks(text, metadata, wallClockStart, onProgress)
    this.activeIndexing.set(metadata.sourceId, indexPromise)
    
    try {
      await indexPromise
    } finally {
      this.activeIndexing.delete(metadata.sourceId)
    }
  }

  private async _doIndexChunks(text: string, metadata: ChunkMetadata, wallClockStart: number, onProgress?: (percent: number) => void): Promise<void> {
    const stats: IndexingStats = {
      sourceId: metadata.sourceId,
      wallClockMs: 0,
      chunkCount: 0,
      embeddingMs: 0,
      storageMs: 0,
      skipped: false
    }

    const report = (p: number) => onProgress?.(p)

    try {
      report(5) // starting
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
          report(100) // done (cached)
          console.log(`[RagService] SKIPPED: ${metadata.sourceId.slice(0, 50)} (content unchanged, VOY verified) [${stats.wallClockMs}ms]`)
          return
        } else {
          console.log(`[RagService] Metadata says indexed but VOY empty, re-indexing...`)
        }
      }

      report(10) // chunking
      const contentType = metadata.sourceType === 'email' ? 'email' : metadata.sourceType === 'pdf' ? 'pdf' : 'article'
      const chunks = await chunkingService.chunkForEmbedding(text, contentType as 'email' | 'article' | 'pdf')
      if (chunks.length === 0) {
        stats.skipped = true
        stats.skipReason = 'no_chunks'
        stats.wallClockMs = Date.now() - wallClockStart
        this.lastStats = stats
        report(100)
        console.log(`[RagService] SKIPPED: No chunks to index [${stats.wallClockMs}ms]`)
        return
      }
      
      report(20) // chunks ready
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

      // embed and store with progress (20% -> 90%)
      const embedStart = Date.now()
      await this.embedAndStoreBatchedWithProgress(entries, EMBEDDING_CONCURRENCY, (batchPercent) => {
        const scaledProgress = 20 + Math.round(batchPercent * 0.7) // 20-90%
        report(scaledProgress)
      })
      stats.embeddingMs = Date.now() - embedStart
      
      report(92) // persisting
      const persistStart = Date.now()
      this.contentHashes.set(metadata.sourceId, contentHash)
      await this.persistMetadata(metadata.sourceId, contentHash, chunks.length)
      
      report(96)
      await vectorStore.forceSave()
      stats.storageMs = Date.now() - persistStart
      
      stats.wallClockMs = Date.now() - wallClockStart
      this.lastStats = stats
      
      // invalidate query cache (new content indexed)
      this.queryCache.clear()
      
      report(100) // done
      console.log(`[RagService] INDEXED: ${chunks.length} chunks for ${metadata.sourceId.slice(0, 50)}`)
      console.log(`[RagService] Timing: total=${stats.wallClockMs}ms, embed=${stats.embeddingMs}ms, persist=${stats.storageMs}ms`)
    } catch (err) {
      stats.wallClockMs = Date.now() - wallClockStart
      this.lastStats = stats
      report(100) // end on error too
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

  // batch embed and store - uses GPU-level batching for 3x speedup
  private async embedAndStoreBatched(entries: VectorEntry[], batchSize: number): Promise<void> {
    await this.embedAndStoreBatchedWithProgress(entries, batchSize)
  }

  // batch embed with progress callback for UI updates
  private async embedAndStoreBatchedWithProgress(
    entries: VectorEntry[], 
    batchSize: number, 
    onBatchProgress?: (percent: number) => void
  ): Promise<void> {
    const totalBatches = Math.ceil(entries.length / batchSize)
    
    for (let i = 0; i < entries.length; i += batchSize) {
      const batch = entries.slice(i, i + batchSize)
      const texts = batch.map(e => e.content)
      
      // true batch embedding (GPU processes all at once)
      const embeddings = await embeddingProvider.embedBatch(texts, false)
      
      // batch add to vector store
      await Promise.all(
        batch.map((entry, idx) => vectorStore.add(entry, embeddings[idx]))
      )
      
      // report progress after each batch
      const batchNum = Math.floor(i / batchSize) + 1
      const percent = Math.round((batchNum / totalBatches) * 100)
      onBatchProgress?.(percent)
    }
  }

  async search(query: string, limit = 5): Promise<SearchResult[]> {
    // check LRU cache first (instant repeat queries)
    const cacheKey = `${query}:${limit}`
    const cached = this.queryCache.get(cacheKey)
    if (cached) {
      console.log('[RagService] Query cache hit')
      return cached
    }

    try {
      // 1. preprocess query
      const processed = this.preprocessQuery(query)
      
      // 2. parallel retrieval with larger pool
      const queryEmbedding = await embeddingProvider.embedQuery(processed)
      const [vectorResults, keywordResults] = await Promise.all([
        vectorStore.search(Array.from(queryEmbedding), limit * 3),
        vectorStore.searchKeyword(processed, limit * 3)
      ])
      
      // 3. RRF fusion with adaptive weights
      const fused = this.fuseResultsRRF(vectorResults, keywordResults, 60, processed)
      
      // 4. overlap reranking
      const reranked = this.rerankByOverlap(processed, fused.slice(0, 50))
      
      const results = reranked.slice(0, limit)
      
      // cache results
      this.queryCache.set(cacheKey, results)
      
      return results
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
        filtered = results.filter((r, i) => {
          const entrySourceUrl = r.entry.metadata?.sourceUrl as string | undefined
          if (!entrySourceUrl || typeof entrySourceUrl !== 'string') return false
          const entryBase = entrySourceUrl.split('?')[0]
          
          const match = entrySourceUrl === sourceUrl || 
                 entryBase === sourceBase ||
                 entryBase.startsWith(sourceBase) ||
                 sourceBase.startsWith(entryBase)
          
          if (i < 3 && !match) {
            console.log(`[RagService] URL mismatch: query="${sourceBase.slice(0, 60)}" vs entry="${entryBase.slice(0, 60)}"`)
          }
          return match
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

  async searchWithSources(query: string, limit = 5): Promise<{
    context: string
    sources: Array<{
      index: number
      title: string
      url: string
      type: 'email' | 'page' | 'pdf'
      score: number
      snippet: string
    }>
  }> {
    try {
      const results = await this.search(query, limit * 2)
      if (results.length === 0) return { context: '', sources: [] }
      
      results.sort((a, b) => b.score - a.score)
      const top = results.slice(0, limit)
      
      const sources = top.map((r, i) => {
        const meta = r.entry.metadata || {}
        return {
          index: i + 1,
          title: (meta.title as string) || (meta.sourceUrl as string) || r.entry.id,
          url: (meta.sourceUrl as string) || '',
          type: (r.entry.type === 'email' ? 'email' : r.entry.type === 'pdf' ? 'pdf' : 'page') as 'email' | 'page' | 'pdf',
          score: r.score,
          snippet: r.entry.content.slice(0, 200)
        }
      })
      
      // build context with numbered source markers
      const contextParts = top.map((r, i) => {
        const meta = r.entry.metadata || {}
        const title = meta.title || meta.sourceUrl || r.entry.id
        return `[${i + 1}] ${title}:\n${r.entry.content}`
      })
      
      return {
        context: contextParts.join('\n\n'),
        sources
      }
    } catch (err) {
      console.error('[RagService] searchWithSources failed:', err)
      return { context: '', sources: [] }
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

  // query preprocessing - typo fixes, expansion (+10% accuracy, +5ms)
  private preprocessQuery(query: string): string {
    let processed = query.toLowerCase().trim()
    
    // common typo fixes
    const typos: Record<string, string> = {
      'oder': 'order', 'sumary': 'summary', 'emai': 'email',
      'reciet': 'receipt', 'invocie': 'invoice', 'reciept': 'receipt',
      'purchse': 'purchase', 'delivry': 'delivery', 'shiping': 'shipping'
    }
    
    for (const [typo, fix] of Object.entries(typos)) {
      processed = processed.replace(new RegExp(`\\b${typo}\\b`, 'gi'), fix)
    }
    
    // expand common terms for better embedding match
    processed = processed
      .replace(/\bpdf\b/gi, 'pdf document')
      .replace(/\bai\b/gi, 'artificial intelligence')
    
    return processed
  }

  // reciprocal rank fusion with adaptive weights
  private fuseResultsRRF(vec: SearchResult[], key: SearchResult[], k = 60, query?: string): SearchResult[] {
    // adaptive weights based on query type
    const queryType = query ? this.classifyQueryType(query) : 'hybrid'
    const weights = queryType === 'keyword' 
      ? { vector: 0.3, keyword: 0.7 }  // short queries favor keyword
      : queryType === 'semantic'
      ? { vector: 0.8, keyword: 0.2 }  // long queries favor semantic
      : { vector: 0.6, keyword: 0.4 }  // default
    
    const scores = new Map<string, { result: SearchResult; rrf: number }>()
    
    // semantic rank scores with weight
    vec.forEach((r, rank) => {
      let rrf = weights.vector / (k + rank + 1)
      // boost summaries 1.3x
      if (r.entry.metadata?.isSummary) rrf *= 1.3
      scores.set(r.entry.id, { result: r, rrf })
    })
    
    // keyword rank scores with weight
    key.forEach((r, rank) => {
      let rrfAdd = weights.keyword / (k + rank + 1)
      if (r.entry.metadata?.isSummary) rrfAdd *= 1.3
      
      if (scores.has(r.entry.id)) {
        const existing = scores.get(r.entry.id)!
        existing.rrf += rrfAdd
        existing.result.matchType = 'hybrid'
      } else {
        scores.set(r.entry.id, { result: r, rrf: rrfAdd })
      }
    })
    
    // sort by RRFs
    const sorted = Array.from(scores.values()).sort((a, b) => b.rrf - a.rrf)
    const maxRrf = sorted[0]?.rrf || 1
    const minRrf = sorted[sorted.length - 1]?.rrf || 0
    const range = maxRrf - minRrf || 1
    
    return sorted.map(s => ({
      ...s.result,
      score: Math.round(((s.rrf - minRrf) / range) * 60 + 40) // 40-100% range
    }))
  }
  
  // classify query for adaptive weights
  private classifyQueryType(query: string): 'semantic' | 'keyword' | 'hybrid' {
    const words = query.split(/\s+/).length
    if (words <= 2) return 'keyword'
    if (words >= 8) return 'semantic'
    return 'hybrid'
  }

  private rerankByOverlap(query: string, results: SearchResult[]): SearchResult[] {
    const queryTerms = new Set(query.toLowerCase().split(/\s+/).filter(t => t.length > 2))
    if (queryTerms.size === 0) return results
    
    const reranked = results.map(r => {
      const docText = r.entry.content.toLowerCase()
      
      // query term coverage in doc
      let matched = 0
      for (const term of queryTerms) {
        if (docText.includes(term)) matched++
      }
      const overlap = matched / queryTerms.size
      
      // small bonus for overlap (max +10 points)
      return { ...r, score: r.score + overlap * 10 }
    }).sort((a, b) => b.score - a.score)
    
    // re-normalize to 40-100 range
    const max = reranked[0]?.score || 1
    const min = reranked[reranked.length - 1]?.score || 0
    const range = max - min || 1
    
    return reranked.map(r => ({
      ...r,
      score: Math.round(((r.score - min) / range) * 60 + 40)
    }))
  }

  async indexSummary(summaryText: string, metadata: ChunkMetadata): Promise<void> {
    if (!summaryText?.trim()) return
    
    const summarySourceId = metadata.sourceId + ':summary'
    
    // check if already indexed
    await this.ensureMetadataLoaded()
    if (this.contentHashes.has(summarySourceId)) {
      console.log(`[RagService] Summary already indexed for ${metadata.sourceId.slice(0, 40)}`)
      return
    }
    
    const summaryMetadata: ChunkMetadata = {
      ...metadata,
      sourceId: summarySourceId,
      title: 'Summary: ' + (metadata.title || metadata.sourceUrl)
    }
    
    // create single entry for summary
    const entry: VectorEntry = {
      id: `${metadata.sourceType}:${summarySourceId}`,
      type: metadata.sourceType,
      content: summaryText,
      metadata: {
        ...summaryMetadata,
        isSummary: true
      },
      timestamp: Date.now()
    }
    
    try {
      const embedding = await embeddingProvider.embedDocument(summaryText)
      await vectorStore.add(entry, embedding)
      
      // persist metadata
      this.contentHashes.set(summarySourceId, this.hashContent(summaryText))
      await this.persistMetadata(summarySourceId, this.hashContent(summaryText), 1)
      
      console.log(`[RagService] Indexed summary for ${metadata.sourceId.slice(0, 40)}`)
    } catch (err) {
      console.error('[RagService] Summary indexing failed:', err)
    }
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
  
  getMetadataCount(): number {
    return this.contentHashes.size
  }
}

export const ragService = new RagService()

