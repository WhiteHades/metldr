// vector store - manages vector embeddings with voy-search
// uses LocalModelProvider for all sandbox operations

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
  private saveDebounceMs = 500
  private loaded = false
  private loadingPromise: Promise<void> | null = null
  private invertedIndex = new InvertedIndex()

  async add(entry: VectorEntry, embedding: Float32Array): Promise<void> {
    this.operationQueue = this.operationQueue.then(async () => {
      await this.ensureIndexLoaded()
      await this.storeDocument(entry)
      this.invertedIndex.add(entry)
      await localModels.voyAdd(entry.id, Array.from(embedding), String(entry.metadata?.title || entry.id), String(entry.metadata?.sourceUrl || ''))
      this.scheduleDebouncedSave()
    }).catch(err => {
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

  private async ensureIndexLoaded() {
    if (this.loaded) return
    if (this.loadingPromise) return this.loadingPromise

    this.loadingPromise = (async () => {
      console.log('[VectorStore] Loading index...')
      const record = await databaseService.get<any>(DB_CONFIGS.cache, 'page_cache', 'system:voy_index')
      if (record && record.value) {
        console.log('[VectorStore] Found persisted index, loading...')
        await localModels.voyLoad(record.value)
        console.log('[VectorStore] Index loaded from IDB.')
      } else {
        console.log('[VectorStore] No persisted index found, starting fresh.')
      }
      
      await this.rebuildInvertedIndex()
      this.loaded = true
    })()

    await this.loadingPromise
  }

  private async rebuildInvertedIndex() {
    console.log('[VectorStore] Rebuilding inverted index...')
    this.invertedIndex.clear()
    const db = await databaseService.getDatabase(DB_CONFIGS.cache)
    const tx = db.transaction('page_cache', 'readonly')
    const store = tx.objectStore('page_cache')
    const cursorRequest = store.openCursor()

    return new Promise<void>((resolve) => {
      cursorRequest.onsuccess = (e) => {
        const cursor = (e.target as IDBRequest).result as IDBCursorWithValue
        if (cursor) {
          const val = cursor.value
          if (typeof val.url === 'string' && val.url.startsWith('rag:')) {
            this.invertedIndex.add(val as VectorEntry)
          }
          cursor.continue()
        } else {
          console.log('[VectorStore] Inverted index rebuilt.')
          resolve()
        }
      }
      cursorRequest.onerror = () => resolve()
    })
  }

  private scheduleDebouncedSave() {
    if (this.pendingSaveTimeout) clearTimeout(this.pendingSaveTimeout)
    this.pendingSaveTimeout = setTimeout(() => {
      this.saveIndex()
      this.pendingSaveTimeout = null
    }, this.saveDebounceMs)
  }

  private async saveIndex() {
    console.log('[VectorStore] Saving index...')
    try {
      const index = await localModels.voySerialize()
      if (index) {
        await databaseService.put(DB_CONFIGS.cache, 'page_cache', {
          url: 'system:voy_index',
          value: index,
          timestamp: Date.now()
        })
        console.log('[VectorStore] Index saved.')
      }
    } catch (err) {
      console.error('[VectorStore] Save failed:', err)
    }
  }

  private async storeDocument(entry: VectorEntry): Promise<void> {
    await databaseService.put(DB_CONFIGS.cache, 'page_cache', {
      url: `rag:${entry.id}`,
      ...entry,
      timestamp: Date.now(),
      ttl: 24 * 60 * 60 * 1000 * 30
    })
  }

  private async retrieveDocument(id: string): Promise<VectorEntry | undefined> {
    const item = await databaseService.get<any>(DB_CONFIGS.cache, 'page_cache', `rag:${id}`)
    return item as VectorEntry | undefined
  }
}

export const vectorStore = new VectorStore()
