const DB_CACHE = 'metldr_cache'
const DB_VERSION = 4
const STORE_SUMMARIES = 'summaries'
const STORE_PAGE_CACHE = 'page_cache'
const STORE_REPLY_SUGGESTIONS = 'reply_suggestions'

export class CacheService {
  private db: IDBDatabase | null = null

  async init(): Promise<void> {
    if (this.db) return

    return new Promise<void>((resolve, reject) => {
      const req = indexedDB.open(DB_CACHE, DB_VERSION)

      req.onerror = () => {
        console.error('[CacheService] open failed:', req.error)
        reject(req.error)
      }

      req.onsuccess = () => {
        this.db = req.result
        resolve()
      }

      req.onupgradeneeded = (event) => {
        const target = event.target as IDBOpenDBRequest
        const db = target.result
        if (!db.objectStoreNames.contains(STORE_SUMMARIES)) {
          db.createObjectStore(STORE_SUMMARIES, { keyPath: 'emailId' })
        }
        if (!db.objectStoreNames.contains(STORE_PAGE_CACHE)) {
          db.createObjectStore(STORE_PAGE_CACHE, { keyPath: 'url' })
        }
        if (!db.objectStoreNames.contains(STORE_REPLY_SUGGESTIONS)) {
          db.createObjectStore(STORE_REPLY_SUGGESTIONS, { keyPath: 'emailId' })
        }
      }
    })
  }

  async getEmailSummary(emailId: string): Promise<unknown> {
    if (!this.db) await this.init()

    return new Promise((resolve) => {
      try {
        const tx = this.db!.transaction([STORE_SUMMARIES], 'readonly')
        const store = tx.objectStore(STORE_SUMMARIES)
        const req = store.get(emailId)

        req.onsuccess = () => {
          const result = req.result
          if (!result) {
            resolve(null)
            return
          }

          if (result.timestamp && result.ttl) {
            const now = Date.now()
            if (now > result.timestamp + result.ttl) {
              this.deleteEmailSummary(emailId).catch(() => {})
              resolve(null)
              return
            }
          }

          resolve(result.summary)
        }

        req.onerror = () => resolve(null)
      } catch (err) {
        console.error('[CacheService.getEmailSummary]', (err as Error).message)
        resolve(null)
      }
    })
  }

  async setEmailSummary(emailId: string, summary: unknown, ttlMs = 7 * 24 * 60 * 60 * 1000): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_SUMMARIES], 'readwrite')
      const store = tx.objectStore(STORE_SUMMARIES)
      store.put({
        emailId,
        summary,
        timestamp: Date.now(),
        ttl: ttlMs
      })
      
      chrome.runtime.sendMessage({ type: 'SUMMARY_ADDED', emailId }).catch(() => {})
    } catch (err) {
      console.error('[CacheService.setEmailSummary]', (err as Error).message)
    }
  }

  async deleteEmailSummary(emailId: string): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_SUMMARIES], 'readwrite')
      const store = tx.objectStore(STORE_SUMMARIES)
      store.delete(emailId)
    } catch (err) {
      console.error('[CacheService.deleteEmailSummary]', (err as Error).message)
    }
  }

  async getPageSummary(url: string): Promise<unknown> {
    if (!this.db) await this.init()

    return new Promise((resolve) => {
      try {
        const tx = this.db!.transaction([STORE_PAGE_CACHE], 'readonly')
        const store = tx.objectStore(STORE_PAGE_CACHE)
        const req = store.get(url)

        req.onsuccess = () => {
          const result = req.result
          if (!result) {
            resolve(null)
            return
          }

          if (result.timestamp && result.ttl) {
            const now = Date.now()
            if (now > result.timestamp + result.ttl) {
              this.deletePageSummary(url).catch(() => {})
              resolve(null)
              return
            }
          }

          resolve(result.summary)
        }

        req.onerror = () => resolve(null)
      } catch (err) {
        console.error('[CacheService.getPageSummary]', (err as Error).message)
        resolve(null)
      }
    })
  }

  async setPageSummary(url: string, summary: unknown, ttlSeconds = 3600): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_PAGE_CACHE], 'readwrite')
      const store = tx.objectStore(STORE_PAGE_CACHE)
      store.put({
        url,
        summary,
        timestamp: Date.now(),
        ttl: ttlSeconds * 1000
      })
    } catch (err) {
      console.error('[CacheService.setPageSummary]', (err as Error).message)
    }
  }

  async deletePageSummary(url: string): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_PAGE_CACHE], 'readwrite')
      const store = tx.objectStore(STORE_PAGE_CACHE)
      store.delete(url)
    } catch (err) {
      console.error('[CacheService.deletePageSummary]', (err as Error).message)
    }
  }

  async clearAll(): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_SUMMARIES, STORE_PAGE_CACHE, STORE_REPLY_SUGGESTIONS], 'readwrite')
      tx.objectStore(STORE_SUMMARIES).clear()
      tx.objectStore(STORE_PAGE_CACHE).clear()
      tx.objectStore(STORE_REPLY_SUGGESTIONS).clear()
    } catch (err) {
      console.error('[CacheService.clearAll]', (err as Error).message)
    }
  }

  async getReplySuggestions(emailId: string): Promise<unknown> {
    if (!this.db) await this.init()
    
    if (!this.db!.objectStoreNames.contains(STORE_REPLY_SUGGESTIONS)) {
      console.warn('[CacheService] reply_suggestions store not found')
      return null
    }

    return new Promise((resolve) => {
      try {
        const tx = this.db!.transaction([STORE_REPLY_SUGGESTIONS], 'readonly')
        const store = tx.objectStore(STORE_REPLY_SUGGESTIONS)
        const req = store.get(emailId)

        req.onsuccess = () => {
          const result = req.result
          if (!result) {
            resolve(null)
            return
          }

          if (result.timestamp && result.ttl) {
            const now = Date.now()
            if (now > result.timestamp + result.ttl) {
              this.deleteReplySuggestions(emailId).catch(() => {})
              resolve(null)
              return
            }
          }

          resolve(result.suggestions)
        }

        req.onerror = () => resolve(null)
      } catch (err) {
        console.error('[CacheService.getReplySuggestions]', (err as Error).message)
        resolve(null)
      }
    })
  }

  async setReplySuggestions(emailId: string, suggestions: unknown, ttlMs = 7 * 24 * 60 * 60 * 1000): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_REPLY_SUGGESTIONS], 'readwrite')
      const store = tx.objectStore(STORE_REPLY_SUGGESTIONS)
      store.put({
        emailId,
        suggestions,
        timestamp: Date.now(),
        ttl: ttlMs
      })
    } catch (err) {
      console.error('[CacheService.setReplySuggestions]', (err as Error).message)
    }
  }

  async deleteReplySuggestions(emailId: string): Promise<void> {
    if (!this.db) await this.init()

    try {
      const tx = this.db!.transaction([STORE_REPLY_SUGGESTIONS], 'readwrite')
      const store = tx.objectStore(STORE_REPLY_SUGGESTIONS)
      store.delete(emailId)
    } catch (err) {
      console.error('[CacheService.deleteReplySuggestions]', (err as Error).message)
    }
  }
}

export const cacheService = new CacheService()
