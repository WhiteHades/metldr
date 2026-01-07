// vector store - manages vector embeddings with voy-search
// uses LocalModelProvider for all sandbox operations
// critical: handles sandbox lifecycle (reload voy index when sandbox restarts)

import { databaseService, DB_CONFIGS } from '../DatabaseService'
import { localModels } from '../ai/LocalModelProvider'
import type { VectorEntry, SearchResult } from '../../types'

class InvertedIndex {
  private index: Map<string, Set<string>> = new Map()
  private docs: Map<string, VectorEntry> = new Map()

  add(entry: VectorEntry) {
    this.docs.set(entry.id, entry)
    const tokens = this.tokenize(entry.content)
    for (const token of tokens) {
      if (!this.index.has(token)) this.index.set(token, new Set())
      this.index.get(token)!.add(entry.id)
    }
  }

  search(query: string, limit: number): SearchResult[] {
    const tokens = this.tokenize(query)
    if (tokens.length === 0) return []
    
    const scores = new Map<string, number>()
    for (const token of tokens) {
      const docIds = this.index.get(token)
      if (docIds) {
        const idf = Math.log(this.docs.size / docIds.size + 1)
        for (const id of docIds) {
          scores.set(id, (scores.get(id) || 0) + idf)
        }
      }
    }
    
    return Array.from(scores.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, limit)
      .map(([id, score]) => ({
        entry: this.docs.get(id)!,
        score: score / tokens.length,
        matchType: 'keyword' as const
      }))
      .filter(r => r.entry)
  }

  clear() {
    this.index.clear()
    this.docs.clear()
  }

  getDocCount(): number {
    return this.docs.size
  }

  private tokenize(text: string): string[] {
    return text.toLowerCase()
      .replace(/[^\w\s]/g, ' ')
      .split(/\s+/)
      .filter(t => t.length > 2)
  }
}

export class VectorStore {
  private operationQueue: Promise<void> = Promise.resolve()
  private pendingSaveTimeout: ReturnType<typeof setTimeout> | null = null
  private saveDebounceMs = 300
  private loaded = false
  private loadingPromise: Promise<void> | null = null
  private invertedIndex = new InvertedIndex()
  private pendingAdds = 0
  
  // sandbox lifecycle tracking
  private lastSandboxId: string | null = null
  private voyIndexLoadedAt: number = 0

  async add(entry: VectorEntry, embedding: Float32Array): Promise<void> {
    this.pendingAdds++
    this.operationQueue = this.operationQueue.then(async () => {
      await this.ensureIndexLoaded()
      await this.storeDocument(entry)
      this.invertedIndex.add(entry)
      await localModels.voyAdd(entry.id, Array.from(embedding), String(entry.metadata?.title || entry.id), String(entry.metadata?.sourceUrl || ''))
      this.pendingAdds--
      this.scheduleDebouncedSave()
    }).catch(err => {
      this.pendingAdds--
      console.error('[VectorStore] Add failed:', err)
    })
    return this.operationQueue
  }

  async search(queryEmbedding: number[], limit: number): Promise<SearchResult[]> {
    await this.ensureIndexLoaded()
    const results = await localModels.voySearch(queryEmbedding, limit)

    const hydrated = await Promise.all(results.map(async r => {
      const entry = await this.retrieveDocument(r.id)
      if (!entry) return null
      return { entry, score: r.score, matchType: 'semantic' } as SearchResult
    }))

    return hydrated.filter(Boolean) as SearchResult[]
  }

  async searchKeyword(query: string, limit: number): Promise<SearchResult[]> {
    await this.ensureIndexLoaded()
    return this.invertedIndex.search(query, limit)
  }

  async addBatch(entries: Array<{ entry: VectorEntry; embedding: Float32Array }>): Promise<void> {
    console.log(`[VectorStore] Batch adding ${entries.length} entries...`)
    for (const { entry, embedding } of entries) {
      await this.add(entry, embedding)
    }
    if (this.pendingSaveTimeout) {
      clearTimeout(this.pendingSaveTimeout)
      this.pendingSaveTimeout = null
    }
    await this.saveIndex()
    console.log(`[VectorStore] Batch add complete.`)
  }

  // check if sandbox was recreated and we need to reload voy index
  private async checkSandboxLifecycle(): Promise<boolean> {
    try {
      const currentSandboxId = localModels.getSandboxId()
      if (this.lastSandboxId && currentSandboxId !== this.lastSandboxId) {
        console.log('[VectorStore] Sandbox was recreated, need to reload VOY index')
        this.lastSandboxId = currentSandboxId
        return true
      }
      this.lastSandboxId = currentSandboxId
      return false
    } catch {
      return false
    }
  }

  private async ensureIndexLoaded(): Promise<void> {
    // check if sandbox was recreated (side panel closed/reopened)
    const sandboxRecreated = await this.checkSandboxLifecycle()
    if (sandboxRecreated && this.loaded) {
      console.log('[VectorStore] Forcing reload due to sandbox recreation')
      this.loaded = false
      this.loadingPromise = null
    }
    
    if (this.loaded) return
    if (this.loadingPromise) return this.loadingPromise

    this.loadingPromise = (async () => {
      const startTime = Date.now()
      console.log('[VectorStore] Loading index...')
      
      // load voy index from IDB
      const record = await databaseService.get<any>(DB_CONFIGS.cache, 'page_cache', 'system:voy_index')
      if (record && record.value) {
        console.log('[VectorStore] Found persisted VOY index, loading into sandbox...')
        try {
          // handle both compressed and legacy uncompressed data
          let indexData = record.value as Uint8Array
          if (record.compressed) {
            indexData = await this.decompress(indexData)
            console.log(`[VectorStore] Decompressed VOY index: ${record.value.length} → ${indexData.length} bytes`)
          }
          // convert bytes back to string for voyLoad
          const indexStr = new TextDecoder().decode(indexData)
          await localModels.voyLoad(indexStr)
          this.voyIndexLoadedAt = Date.now()
          console.log(`[VectorStore] VOY index loaded from IDB (${Date.now() - startTime}ms)`)
        } catch (err) {
          console.error('[VectorStore] VOY load failed, starting fresh:', err)
        }
      } else {
        console.log('[VectorStore] No persisted VOY index found, starting fresh.')
      }
      
      // rebuild inverted index from stored documents
      await this.rebuildInvertedIndex()
      
      this.loaded = true
      this.lastSandboxId = localModels.getSandboxId()
      console.log(`[VectorStore] Index ready. ${this.invertedIndex.getDocCount()} docs in inverted index. (${Date.now() - startTime}ms total)`)
    })()

    await this.loadingPromise
  }

  // force reload from IDB - call when index seems stale
  async forceReload(): Promise<void> {
    console.log('[VectorStore] Force reloading index...')
    this.loaded = false
    this.loadingPromise = null
    await this.ensureIndexLoaded()
  }

  // check if we have a specific document indexed
  async hasDocument(sourceId: string): Promise<boolean> {
    await this.ensureIndexLoaded()
    
    // check inverted index first (fast)
    const keywordResults = this.invertedIndex.search(sourceId, 1)
    if (keywordResults.some(r => r.entry.metadata?.sourceId === sourceId || r.entry.metadata?.sourceUrl === sourceId)) {
      return true
    }
    
    // fallback: check IDB for all source types
    for (const type of ['article', 'pdf', 'email']) {
      const doc = await databaseService.get<any>(DB_CONFIGS.cache, 'page_cache', `rag:${type}:${sourceId}:chunk:0`)
      if (doc) return true
    }
    return false
  }

  // get doc count for verification
  getDocCount(): number {
    return this.invertedIndex.getDocCount()
  }

  private async rebuildInvertedIndex(): Promise<void> {
    console.log('[VectorStore] Rebuilding inverted index from IDB...')
    this.invertedIndex.clear()
    const db = await databaseService.getDatabase(DB_CONFIGS.cache)
    const tx = db.transaction('page_cache', 'readonly')
    const store = tx.objectStore('page_cache')
    const cursorRequest = store.openCursor()

    return new Promise<void>((resolve) => {
      let count = 0
      cursorRequest.onsuccess = (e) => {
        const cursor = (e.target as IDBRequest).result as IDBCursorWithValue
        if (cursor) {
          const val = cursor.value
          if (typeof val.url === 'string' && val.url.startsWith('rag:')) {
            this.invertedIndex.add(val as VectorEntry)
            count++
          }
          cursor.continue()
        } else {
          console.log(`[VectorStore] Inverted index rebuilt with ${count} documents.`)
          resolve()
        }
      }
      cursorRequest.onerror = () => {
        console.error('[VectorStore] Cursor error during rebuild')
        resolve()
      }
    })
  }

  private scheduleDebouncedSave(): void {
    if (this.pendingSaveTimeout) clearTimeout(this.pendingSaveTimeout)
    
    const delay = this.pendingAdds > 0 ? this.saveDebounceMs : 50
    this.pendingSaveTimeout = setTimeout(() => {
      this.saveIndex()
      this.pendingSaveTimeout = null
    }, delay)
  }

  async forceSave(): Promise<void> {
    if (this.pendingSaveTimeout) {
      clearTimeout(this.pendingSaveTimeout)
      this.pendingSaveTimeout = null
    }
    await this.saveIndex()
  }

  private async saveIndex(): Promise<void> {
    if (!this.loaded) return
    
    console.log('[VectorStore] Saving VOY index (compressed) to IDB...')
    try {
      // voySerialize now returns string directly (survives chrome messaging)
      const serializedStr = await localModels.voySerialize()
      
      if (!serializedStr) {
        console.log('[VectorStore] VOY index empty, skipping save')
        return
      }
      
      // convert string to bytes for compression
      const index = new TextEncoder().encode(serializedStr)
      
      // gzip compress for 60-80% smaller storage
      const compressed = await this.compress(index)
      
      await databaseService.put(DB_CONFIGS.cache, 'page_cache', {
        url: 'system:voy_index',
        value: compressed,
        compressed: true,
        timestamp: Date.now()
      })
      console.log(`[VectorStore] VOY index saved: ${index.length} → ${compressed.length} bytes (${Math.round((1 - compressed.length / index.length) * 100)}% reduction)`)
    } catch (err) {
      console.error('[VectorStore] Save failed:', err)
    }
  }

  // gzip compression
  private async compress(data: Uint8Array): Promise<Uint8Array> {
    const stream = new Blob([data]).stream().pipeThrough(new CompressionStream('gzip'))
    return new Uint8Array(await new Response(stream).arrayBuffer())
  }

  private async decompress(data: Uint8Array): Promise<Uint8Array> {
    const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream('gzip'))
    return new Uint8Array(await new Response(stream).arrayBuffer())
  }

  private async storeDocument(entry: VectorEntry): Promise<void> {
    await databaseService.put(DB_CONFIGS.cache, 'page_cache', {
      url: `rag:${entry.id}`,
      ...entry,
      timestamp: Date.now(),
      ttl: 24 * 60 * 60 * 1000 * 60 // 60 days
    })
  }

  private async retrieveDocument(id: string): Promise<VectorEntry | undefined> {
    const item = await databaseService.get<any>(DB_CONFIGS.cache, 'page_cache', `rag:${id}`)
    return item as VectorEntry | undefined
  }
}

export const vectorStore = new VectorStore()
