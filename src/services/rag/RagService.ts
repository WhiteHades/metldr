import { vectorStore } from './VectorStore'
import { embeddingProvider } from './EmbeddingProvider'
import { chunkingService } from './ChunkingService'
import { databaseService, DB_CONFIGS } from '../DatabaseService'
import { aiGateway } from '../ai/AIGateway'
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
      // 1. expand query with LLM (for global search - adds related terms)
      const expanded = await this.expandQueryWithLLM(query)
      const processed = this.preprocessQuery(expanded)
      
      // 2. parallel retrieval
      const queryEmbedding = await embeddingProvider.embedQuery(processed)
      const [vectorResults, keywordResults] = await Promise.all([
        vectorStore.search(Array.from(queryEmbedding), limit * 5),
        vectorStore.searchKeyword(processed, limit * 5)
      ])
      
      // 3. RRF fusion with adaptive weights
      const fused = this.fuseResultsByScore(vectorResults, keywordResults, processed)
      
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

  // llm-based query expansion for global search with fallback
  private async expandQueryWithLLM(query: string): Promise<string> {
    // fallback: simple keyword expansion without LLM
    const fallbackExpand = (q: string): string => {
      const synonyms: Record<string, string[]> = {
        'order': ['purchase', 'bought', 'confirmation', 'shipping'],
        'orders': ['purchases', 'bought', 'confirmations', 'shipping'],
        'invoice': ['bill', 'receipt', 'payment', 'statement'],
        'invoices': ['bills', 'receipts', 'payments', 'statements'],
        'email': ['mail', 'message', 'notification'],
        'emails': ['mails', 'messages', 'notifications'],
        'article': ['post', 'blog', 'news', 'story'],
        'articles': ['posts', 'blogs', 'news', 'stories']
      }
      const words = q.toLowerCase().split(/\s+/)
      const extras: string[] = []
      for (const w of words) {
        if (synonyms[w]) extras.push(...synonyms[w])
      }
      return extras.length > 0 ? `${q} ${extras.join(' ')}` : q
    }

    try {
      const response = await aiGateway.complete({
        systemPrompt: `You are a search query expander. Output ONLY comma-separated search terms, no explanation.
Generate 10-15 diverse terms: synonyms, related concepts, different phrasings.`,
        userPrompt: `Query: "${query}"`,
        maxTokens: 100
      })
      
      if (response.ok && response.content) {
        const expanded = response.content
          .replace(/[\"\'\n]/g, '')
          .split(/[,;]/)
          .map((t: string) => t.trim().toLowerCase())
          .filter((t: string) => t.length > 2 && t.length < 40 && !t.includes(':'))
          .slice(0, 12)
        
        if (expanded.length > 0) {
          const result = `${query} ${expanded.join(' ')}`
          console.log(`[RagService] LLM expanded: "${query}" → +${expanded.length} terms`)
          return result
        }
      }
    } catch (err) {
      console.log('[RagService] LLM expansion failed, using fallback:', err)
    }
    
    // use fallback expansion
    return fallbackExpand(query)
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
      // fetch more results for comprehensive coverage
      const results = await this.search(query, limit * 4)
      console.log('[RagService.searchWithSources] search results:', results.length)
      if (results.length === 0) return { context: '', sources: [] }
      
      // extract significant query terms
      const queryTerms = this.extractQueryKeywords(query)
      
      // score results with keyword boost
      const scored = results.map(r => {
        const content = r.entry.content.toLowerCase()
        const title = ((r.entry.metadata?.title as string) || '').toLowerCase()
        const text = content + ' ' + title
        
        let matchedTerms = 0
        for (const term of queryTerms) {
          if (text.includes(term)) matchedTerms++
        }
        const keywordScore = queryTerms.length > 0 ? matchedTerms / queryTerms.length : 0.5
        const boost = 1 + keywordScore
        const adjustedScore = Math.min(100, Math.round(r.score * boost))
        
        return { ...r, score: adjustedScore, keywordScore, matchedTerms }
      })
      
      scored.sort((a, b) => b.score - a.score)
      
      // dedupe by source URL (keep highest-scored chunk per source)
      const seenUrls = new Set<string>()
      const deduped = scored.filter(r => {
        const url = (r.entry.metadata?.sourceUrl as string) || r.entry.id
        const baseUrl = url.split(':chunk:')[0].split('?')[0].replace(/\/$/, '')
        if (seenUrls.has(baseUrl)) return false
        seenUrls.add(baseUrl)
        return true
      })
      
      // take top results without aggressive score filtering
      // just use the limit directly - let the LLM decide relevance
      const top = deduped.slice(0, limit)
      
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
      
      const contextParts = top.map((r, i) => {
        const meta = r.entry.metadata || {}
        const title = meta.title || meta.sourceUrl || r.entry.id
        return `[${i + 1}] ${title}:\n${r.entry.content}`
      })
      
      console.log('[RagService.searchWithSources] returning sources:', sources.length, 'context length:', contextParts.join('').length)
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

  // basic query preprocessing (LLM handles semantic expansion)
  private preprocessQuery(query: string): string {
    return query.toLowerCase().trim()
  }

  // extract significant keywords for strict matching
  private extractQueryKeywords(query: string): string[] {
    const stopwords = new Set([
      'the', 'a', 'an', 'and', 'or', 'but', 'in', 'on', 'at', 'to', 'for',
      'of', 'with', 'by', 'from', 'as', 'is', 'was', 'are', 'were', 'been',
      'be', 'have', 'has', 'had', 'do', 'does', 'did', 'will', 'would', 'could',
      'should', 'may', 'might', 'must', 'can', 'this', 'that', 'these', 'those',
      'i', 'me', 'my', 'you', 'your', 'we', 'our', 'they', 'their', 'it', 'its',
      'what', 'which', 'who', 'whom', 'where', 'when', 'why', 'how',
      'all', 'each', 'every', 'both', 'few', 'more', 'most', 'other', 'some',
      'any', 'no', 'not', 'only', 'own', 'same', 'so', 'than', 'too', 'very',
      'just', 'also', 'now', 'here', 'there', 'then', 'once', 'again'
    ])
    
    return query.toLowerCase()
      .split(/\s+/)
      .filter(t => t.length > 2 && !stopwords.has(t))
  }

  private fuseResultsByScore(vec: SearchResult[], key: SearchResult[], query?: string): SearchResult[] {
    const queryType = query ? this.classifyQueryType(query) : 'hybrid'
    
    const vecIds = new Set(vec.map(r => r.entry.id))
    const keyIds = new Set(key.map(r => r.entry.id))
    const overlapCount = [...vecIds].filter(id => keyIds.has(id)).length
    const overlapRatio = Math.max(vecIds.size, keyIds.size) > 0 
      ? overlapCount / Math.max(vecIds.size, keyIds.size) 
      : 0
    
    // adaptive weights: high overlap = trust both, low overlap = boost the method with better coverage
    let weights: { vector: number; keyword: number }
    if (overlapRatio > 0.5) {
      // high agreement  trust both equally, hybrid matches are very reliable
      weights = { vector: 0.5, keyword: 0.5 }
    } else if (queryType === 'keyword') {
      weights = { vector: 0.35, keyword: 0.65 }
    } else if (queryType === 'semantic') {
      weights = { vector: 0.65, keyword: 0.35 }
    } else {
      weights = { vector: 0.5, keyword: 0.5 }
    }
    
    const scores = new Map<string, { 
      result: SearchResult
      vectorScore: number
      keywordScore: number
      vecRank: number
      keyRank: number
    }>()
    
    const vecScores = vec.map(r => r.score)
    const keyScores = key.map(r => r.score)
    const vecMean = vecScores.length > 0 ? vecScores.reduce((a,b) => a+b, 0) / vecScores.length : 0
    const keyMean = keyScores.length > 0 ? keyScores.reduce((a,b) => a+b, 0) / keyScores.length : 0
    const vecStd = Math.sqrt(vecScores.reduce((sum, s) => sum + (s - vecMean) ** 2, 0) / (vecScores.length || 1)) || 1
    const keyStd = Math.sqrt(keyScores.reduce((sum, s) => sum + (s - keyMean) ** 2, 0) / (keyScores.length || 1)) || 1
    
    // add vector results with zscore normalization mapped to 0-100
    vec.forEach((r, rank) => {
      const zScore = (r.score - vecMean) / vecStd
      const normalizedScore = Math.min(100, Math.max(0, 50 + zScore * 25))
      scores.set(r.entry.id, { 
        result: r, 
        vectorScore: normalizedScore,
        keywordScore: 0,
        vecRank: rank,
        keyRank: -1
      })
    })
    
    // merge keyword results
    key.forEach((r, rank) => {
      const zScore = (r.score - keyMean) / keyStd
      const normalizedScore = Math.min(100, Math.max(0, 50 + zScore * 25))
      if (scores.has(r.entry.id)) {
        const existing = scores.get(r.entry.id)!
        existing.keywordScore = normalizedScore
        existing.keyRank = rank
        existing.result.matchType = 'hybrid'
      } else {
        scores.set(r.entry.id, { 
          result: r, 
          vectorScore: 0,
          keywordScore: normalizedScore,
          vecRank: -1,
          keyRank: rank
        })
      }
    })
    
    const results = Array.from(scores.values()).map(s => {
      const weightedScore = s.vectorScore * weights.vector + s.keywordScore * weights.keyword
      
      let hybridBonus = 0
      if (s.vecRank >= 0 && s.keyRank >= 0) {
        const rankDiff = Math.abs(s.vecRank - s.keyRank)
        hybridBonus = Math.max(5, 25 - rankDiff * 3)
      }

      const summaryBoost = s.result.entry.metadata?.isSummary ? 8 : 0
      
      const finalScore = Math.min(100, weightedScore + hybridBonus + summaryBoost)
      
      return {
        ...s.result,
        score: Math.round(finalScore),
        sourceUrl: s.result.entry.metadata?.sourceUrl as string || ''
      }
    })
    
    // sort by score
    results.sort((a, b) => b.score - a.score)
    
    const seenSources = new Map<string, number>()
    const diverseResults = results.map(r => {
      const sourceBase = r.sourceUrl.split('?')[0].split('#')[0]
      const occurrences = seenSources.get(sourceBase) || 0
      seenSources.set(sourceBase, occurrences + 1)
      
      // reduce score for 2nd, 3rd, etc. chunks from same source
      const diversityPenalty = occurrences > 0 ? occurrences * 15 : 0
      
      return {
        ...r,
        score: Math.max(0, r.score - diversityPenalty)
      }
    })
    
    return diverseResults.sort((a, b) => b.score - a.score)
  }
  
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
      
      let matched = 0
      for (const term of queryTerms) {
        if (docText.includes(term)) matched++
      }
      const overlap = matched / queryTerms.size
      
      // bonus for overlap (max +15 points), cap at 100
      return { ...r, score: Math.min(100, r.score + Math.round(overlap * 15)) }
    }).sort((a, b) => b.score - a.score)
    
    return reranked
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

  // unified entry point: check if indexed, wait if in-progress, or index if needed
  async ensureIndexed(
    text: string, 
    metadata: ChunkMetadata, 
    onProgress?: (percent: number) => void
  ): Promise<'already-indexed' | 'was-indexing' | 'indexed-now'> {
    await this.ensureMetadataLoaded()

    // check if already indexed
    const hasContent = await this.hasIndexedContent(metadata.sourceId)
    if (hasContent) {
      console.log(`[RagService] ensureIndexed: already indexed ${metadata.sourceId.slice(0, 50)}`)
      return 'already-indexed'
    }

    // check if indexing in progress
    const existing = this.activeIndexing.get(metadata.sourceId)
    if (existing) {
      console.log(`[RagService] ensureIndexed: waiting on in-progress ${metadata.sourceId.slice(0, 50)}`)
      await existing
      return 'was-indexing'
    }

    await this.indexChunks(text, metadata, onProgress)
    return 'indexed-now'
  }

  async getIndexingStatus(sourceId: string): Promise<'indexed' | 'in-progress' | 'needed'> {
    if (this.activeIndexing.has(sourceId)) return 'in-progress'
    
    await this.ensureMetadataLoaded()
    const hasContent = await this.hasIndexedContent(sourceId)
    return hasContent ? 'indexed' : 'needed'
  }
}

export const ragService = new RagService()

