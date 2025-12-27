import type {
  SupportedLanguage,
  CacheEntry,
  EmailSummaryEntry,
  WordDefinitionEntry,
  DictEntry,
  DictMetaEntry,
  DownloadProgress,
  ResumeState,
  DictLookupResult
} from '../types'

const CACHE_DB_NAME = 'metldr_storage'
const CACHE_DB_VERSION = 1
const DICT_DB_NAME = 'metldr-dictionary'
const DICT_DB_VERSION = 3
const DICT_BASE_URL = 'https://media.githubusercontent.com/media/WhiteHades/wikitionary-dictionary-json/master/dist'

export const SUPPORTED_LANGUAGES: SupportedLanguage[] = [
  { code: 'en', name: 'English', url: `${DICT_BASE_URL}/English` },
  { code: 'es', name: 'Spanish', url: `${DICT_BASE_URL}/Spanish` },
  { code: 'fr', name: 'French', url: `${DICT_BASE_URL}/French` },
  { code: 'de', name: 'German', url: `${DICT_BASE_URL}/German` },
  { code: 'it', name: 'Italian', url: `${DICT_BASE_URL}/Italian` },
  { code: 'pt', name: 'Portuguese', url: `${DICT_BASE_URL}/Portuguese` },
  { code: 'nl', name: 'Dutch', url: `${DICT_BASE_URL}/Dutch` },
  { code: 'sv', name: 'Swedish', url: `${DICT_BASE_URL}/Swedish` },
  { code: 'pl', name: 'Polish', url: `${DICT_BASE_URL}/Polish` },
  { code: 'ro', name: 'Romanian', url: `${DICT_BASE_URL}/Romanian` },
  { code: 'cs', name: 'Czech', url: `${DICT_BASE_URL}/Czech` },
  { code: 'fi', name: 'Finnish', url: `${DICT_BASE_URL}/Finnish` },
  { code: 'da', name: 'Danish', url: `${DICT_BASE_URL}/Danish` },
  { code: 'no', name: 'Norwegian', url: `${DICT_BASE_URL}/Norwegian` },
  { code: 'nb', name: 'Norwegian (Bokmål)', url: `${DICT_BASE_URL}/Norwegian%20(Bokm%C3%A5l)` },
  { code: 'nn', name: 'Norwegian (Nynorsk)', url: `${DICT_BASE_URL}/Norwegian%20(Nynorsk)` },
  { code: 'id', name: 'Indonesian', url: `${DICT_BASE_URL}/Indonesian` },
  { code: 'ms', name: 'Malay', url: `${DICT_BASE_URL}/Malay` },
  { code: 'tr', name: 'Turkish', url: `${DICT_BASE_URL}/Turkish` },
  { code: 'vi', name: 'Vietnamese', url: `${DICT_BASE_URL}/Vietnamese` },
  { code: 'lt', name: 'Lithuanian', url: `${DICT_BASE_URL}/Lithuanian` },
  { code: 'sk', name: 'Slovak', url: `${DICT_BASE_URL}/Slovak` }
]

export class StorageManager {
  private cacheDb: IDBDatabase | null = null
  private dictDb: IDBDatabase | null = null

  async initCache(): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const request = indexedDB.open(CACHE_DB_NAME, CACHE_DB_VERSION)
      request.onerror = () => reject(request.error)
      request.onsuccess = () => {
        this.cacheDb = request.result
        resolve()
      }
      request.onupgradeneeded = (event) => {
        const target = event.target as IDBOpenDBRequest
        const db = target.result
        if (!db.objectStoreNames.contains('ollama_cache')) {
          const ollamaStore = db.createObjectStore('ollama_cache', { keyPath: 'key' })
          ollamaStore.createIndex('timestamp', 'timestamp', { unique: false })
        }
        if (!db.objectStoreNames.contains('email_summaries')) {
          const emailStore = db.createObjectStore('email_summaries', { keyPath: 'emailId' })
          emailStore.createIndex('timestamp', 'timestamp', { unique: false })
        }
        if (!db.objectStoreNames.contains('word_definitions')) {
          db.createObjectStore('word_definitions', { keyPath: 'word' })
        }
      }
    })
  }

  async cacheSet(key: string, value: unknown, ttl = 7 * 24 * 60 * 60 * 1000): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const transaction = this.cacheDb!.transaction(['ollama_cache'], 'readwrite')
    const store = transaction.objectStore('ollama_cache')
    store.put({ key, value, timestamp: Date.now(), ttl })
  }

  async cacheGet(key: string): Promise<unknown> {
    if (!this.cacheDb) await this.initCache()
    return new Promise((resolve) => {
      const transaction = this.cacheDb!.transaction(['ollama_cache'], 'readonly')
      const store = transaction.objectStore('ollama_cache')
      const request = store.get(key)
      request.onsuccess = () => {
        const result = request.result as CacheEntry | undefined
        if (!result) {
          resolve(null)
          return
        }
        const { value, timestamp, ttl } = result
        if (Date.now() > timestamp + ttl) {
          this.cacheDelete(key)
          resolve(null)
          return
        }
        resolve(value)
      }
      request.onerror = () => resolve(null)
    })
  }

  async cacheDelete(key: string): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const transaction = this.cacheDb!.transaction(['ollama_cache'], 'readwrite')
    const store = transaction.objectStore('ollama_cache')
    store.delete(key)
  }

  async emailSummarySet(emailId: string, summary: unknown, metadata: Record<string, unknown> = {}): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const transaction = this.cacheDb!.transaction(['email_summaries'], 'readwrite')
    const store = transaction.objectStore('email_summaries')
    store.put({ emailId, summary, timestamp: Date.now(), ...metadata })
  }

  async emailSummaryGet(emailId: string): Promise<{ summary: unknown; metadata: EmailSummaryEntry } | null> {
    if (!this.cacheDb) await this.initCache()
    return new Promise((resolve) => {
      const transaction = this.cacheDb!.transaction(['email_summaries'], 'readonly')
      const store = transaction.objectStore('email_summaries')
      const request = store.get(emailId)
      request.onsuccess = () => {
        const result = request.result as EmailSummaryEntry | undefined
        resolve(result ? { summary: result.summary, metadata: result } : null)
      }
      request.onerror = () => resolve(null)
    })
  }

  async wordDefinitionSet(word: string, definition: unknown): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const transaction = this.cacheDb!.transaction(['word_definitions'], 'readwrite')
    const store = transaction.objectStore('word_definitions')
    store.put({ word, definition })
  }

  async wordDefinitionGet(word: string): Promise<unknown> {
    if (!this.cacheDb) await this.initCache()
    return new Promise((resolve) => {
      const transaction = this.cacheDb!.transaction(['word_definitions'], 'readonly')
      const store = transaction.objectStore('word_definitions')
      const request = store.get(word)
      request.onsuccess = () => {
        const result = request.result as WordDefinitionEntry | undefined
        resolve(result ? result.definition : null)
      }
      request.onerror = () => resolve(null)
    })
  }

  async cacheClearAll(): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const stores = ['ollama_cache', 'email_summaries', 'word_definitions']
    for (const storeName of stores) {
      const transaction = this.cacheDb!.transaction([storeName], 'readwrite')
      const store = transaction.objectStore(storeName)
      store.clear()
    }
  }

  async cacheCleanup(): Promise<void> {
    if (!this.cacheDb) await this.initCache()
    const transaction = this.cacheDb!.transaction(['ollama_cache'], 'readwrite')
    const store = transaction.objectStore('ollama_cache')
    const index = store.index('timestamp')
    const request = index.openCursor()
    return new Promise<void>((resolve) => {
      request.onsuccess = (event) => {
        const target = event.target as IDBRequest<IDBCursorWithValue | null>
        const cursor = target.result
        if (!cursor) {
          resolve()
          return
        }
        const { timestamp, ttl } = cursor.value as CacheEntry
        if (Date.now() > timestamp + ttl) {
          cursor.delete()
        }
        cursor.continue()
      }
    })
  }

  async initDictionary(): Promise<IDBDatabase> {
    if (this.dictDb) {
      console.log('[dict] already initialized, stores:', Array.from(this.dictDb.objectStoreNames))
      return this.dictDb
    }
    
    console.log('[dict] initializing dictionary db:', DICT_DB_NAME, 'v' + DICT_DB_VERSION)
    return new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DICT_DB_NAME, DICT_DB_VERSION)
      
      request.onerror = () => {
        console.error('[dict] failed to open db:', request.error)
        reject(request.error)
      }
      
      request.onsuccess = () => {
        this.dictDb = request.result
        const stores = Array.from(this.dictDb.objectStoreNames)
        console.log('[dict] db opened successfully, stores:', stores)
        
        if (!stores.includes('meta')) {
          console.warn('[dict] meta store missing - db may need upgrade')
        }
        
        resolve(this.dictDb)
      }
      
      request.onupgradeneeded = (event) => {
        console.log('[dict] upgrading db schema from v' + event.oldVersion + ' to v' + event.newVersion)
        const target = event.target as IDBOpenDBRequest
        const db = target.result
        
        let created = 0
        SUPPORTED_LANGUAGES.forEach(lang => {
          if (!db.objectStoreNames.contains(lang.code)) {
            db.createObjectStore(lang.code, { keyPath: 'word' })
            created++
          }
        })
        
        if (!db.objectStoreNames.contains('meta')) {
          db.createObjectStore('meta', { keyPath: 'key' })
          created++
        }
        
        console.log('[dict] created', created, 'new object stores')
      }
    })
  }

  async dictIsLanguageDownloaded(langCode: string): Promise<boolean> {
    try {
      if (!this.dictDb) await this.initDictionary()
      if (!this.dictDb!.objectStoreNames.contains('meta')) {
        return false
      }
      return new Promise((resolve) => {
        const tx = this.dictDb!.transaction(['meta'], 'readonly')
        const store = tx.objectStore('meta')
        const request = store.get(`lang-${langCode}`)
        request.onsuccess = () => {
          const result = request.result as DictMetaEntry | undefined
          resolve(result?.downloaded === true)
        }
        request.onerror = () => resolve(false)
      })
    } catch (err) {
      console.warn('storagemanager: dictIsLanguageDownloaded failed:', (err as Error).message)
      return false
    }
  }

  async dictMarkLanguageDownloaded(langCode: string): Promise<void> {
    if (!this.dictDb) await this.initDictionary()
    return new Promise<void>((resolve, reject) => {
      const tx = this.dictDb!.transaction(['meta'], 'readwrite')
      const store = tx.objectStore('meta')
      const request = store.put({
        key: `lang-${langCode}`,
        downloaded: true,
        timestamp: Date.now()
      })
      request.onsuccess = () => resolve()
      request.onerror = () => reject(request.error)
    })
  }

  async dictDownloadLanguage(langCode: string, onProgress?: (progress: DownloadProgress) => void): Promise<number> {
    const lang = SUPPORTED_LANGUAGES.find(l => l.code === langCode)
    if (!lang) throw new Error(`language ${langCode} not supported`)
    if (!this.dictDb) await this.initDictionary()

    if (!this.dictDb!.objectStoreNames.contains(langCode)) {
      console.log('[dict] store missing for', langCode, '- recreating db')
      this.dictDb!.close()
      this.dictDb = null
      await this.initDictionary()
      
      if (!this.dictDb!.objectStoreNames.contains(langCode)) {
        throw new Error(`failed to create store for ${langCode}`)
      }
    }

    await this._markDownloading(langCode, true)
    const resumeState = await this._getDownloadProgress(langCode)
    await this._setDownloadProgress(langCode, resumeState?.nextIndex || 0, resumeState?.entriesProcessed || 0)

    try {
      console.log('[dict] starting download for:', langCode, '| base url:', lang.url)
      const letters = 'abcdefghijklmnopqrstuvwxyz'.split('')
      let totalEntries = 0
      let successfulLetters = 0
      let startIndex = 0

      if (resumeState && typeof resumeState.nextIndex === 'number') {
        startIndex = Math.min(Math.max(resumeState.nextIndex, 0), letters.length)
        console.log('[dict] resuming download for', langCode, 'from index', startIndex)
      }

      for (let i = startIndex; i < letters.length; i++) {
        const letter = letters[i]
        const url = `${lang.url}/${letter}.json`
        try {
          console.log('[dict] fetching:', url)
          const response = await fetch(url, { 
            cache: 'no-cache',
            headers: { 'Accept': 'application/json' }
          })
          
          if (!response.ok) {
            console.warn('[dict] fetch failed:', url, '| status:', response.status, response.statusText)
            continue
          }
                    
          const entries = await response.json() as DictEntry[]
          if (!Array.isArray(entries)) {
            console.warn('[dict] unexpected response format for', url)
            continue
          }
          
          console.log('[dict] got', entries.length, 'entries for letter', letter)
          await this.dictBatchInsert(langCode, entries)
          totalEntries += entries.length
          successfulLetters++
          
          if (onProgress) {
            onProgress({
              letter,
              progress: ((i + 1) / letters.length) * 100,
              entriesProcessed: totalEntries
            })
          }
        } catch (err) {
          console.error('[dict] error downloading', letter + '.json for', lang.name, ':', (err as Error).message)
        }
          
        await this._setDownloadProgress(langCode, i + 1, totalEntries)
      }

      console.log('[dict] download complete for:', langCode, '| letters:', successfulLetters + '/26 | entries:', totalEntries)
      
      if (totalEntries > 0) {
        await this.dictMarkLanguageDownloaded(langCode)
      } else {
        console.error('[dict] no entries downloaded for', langCode, '- not marking as complete')
      }
      
      await this._clearDownloadProgress(langCode)
      
      return totalEntries
    } finally {
      await this._markDownloading(langCode, false)
    }
  }

  async _getDownloadProgress(langCode: string): Promise<ResumeState | null> {
    try {
      const result = await chrome.storage.local.get(['dictDownloadProgress'])
      const map = (result.dictDownloadProgress || {}) as Record<string, ResumeState>
      return map[langCode] || null
    } catch (err) {
      console.warn('storagemanager: read progress failed:', (err as Error).message)
      return null
    }
  }

  async _setDownloadProgress(langCode: string, nextIndex: number, entriesProcessed: number): Promise<void> {
    try {
      const result = await chrome.storage.local.get(['dictDownloadProgress'])
      const map = (result.dictDownloadProgress || {}) as Record<string, ResumeState>
      map[langCode] = { nextIndex, entriesProcessed }
      await chrome.storage.local.set({ dictDownloadProgress: map })
    } catch (err) {
      console.warn('storagemanager: save progress failed:', (err as Error).message)
    }
  }

  async _clearDownloadProgress(langCode: string): Promise<void> {
    try {
      const result = await chrome.storage.local.get(['dictDownloadProgress'])
      const map = (result.dictDownloadProgress || {}) as Record<string, ResumeState>
      delete map[langCode]
      await chrome.storage.local.set({ dictDownloadProgress: map })
    } catch (err) {
      console.warn('storagemanager: clear progress failed:', (err as Error).message)
    }
  }

  async _markDownloading(langCode: string, add: boolean): Promise<void> {
    try {
      const result = await chrome.storage.local.get(['downloadingLanguages'])
      const current = Array.isArray(result.downloadingLanguages) ? result.downloadingLanguages as string[] : []
      const set = new Set(current)
      if (add) {
        set.add(langCode)
      } else {
        set.delete(langCode)
      }
      const downloadingLanguages = Array.from(set)
      await chrome.storage.local.set({
        downloadingLanguages,
        dictDownloading: downloadingLanguages.length > 0
      })
    } catch (err) {
      console.warn('storagemanager: failed to mark downloading:', (err as Error).message)
    }
  }

  async dictBatchInsert(langCode: string, entries: DictEntry[]): Promise<void> {
    if (!this.dictDb) await this.initDictionary()
    
    if (!this.dictDb!.objectStoreNames.contains(langCode)) {
      console.error('[dict] store not found:', langCode, '- reinitializing db')
      this.dictDb!.close()
      this.dictDb = null
      await this.initDictionary()
      
      if (!this.dictDb!.objectStoreNames.contains(langCode)) {
        throw new Error(`language store ${langCode} not found after reinit`)
      }
    }
    
    return new Promise<void>((resolve, reject) => {
      try {
        const tx = this.dictDb!.transaction([langCode], 'readwrite')
        const store = tx.objectStore(langCode)
        entries.forEach(entry => {
          store.put({
            word: entry.word.toLowerCase(),
            pos: entry.pos,
            definition: entry.definition
          })
        })
        tx.oncomplete = () => resolve()
        tx.onerror = () => reject(tx.error)
      } catch (err) {
        console.error('[dict] batch insert error:', (err as Error).message)
        reject(err)
      }
    })
  }

  async dictLookupFromAPI(word: string, langCode = 'en'): Promise<DictLookupResult | null> {
    try {
      const url = `https://api.dictionaryapi.dev/api/v2/entries/${langCode}/${word}`
      const response = await fetch(url)
      if (!response.ok) return null
      const data = await response.json()
      if (!data || !data[0] || !data[0].meanings) return null
      const firstMeaning = data[0].meanings[0]
      const firstDef = firstMeaning.definitions[0]
      return {
        word: word.toLowerCase(),
        pos: firstMeaning.partOfSpeech || 'unknown',
        definition: firstDef.definition || '',
        source: 'api'
      }
    } catch (error) {
      console.error('storage manager: API lookup failed:', error)
      return null
    }
  }

  async dictLookup(word: string, langCode = 'en'): Promise<DictLookupResult | null> {
    try {
      if (!this.dictDb) await this.initDictionary()
      if (!this.dictDb!.objectStoreNames.contains(langCode)) {
        return null
      }
      const normalised = word.toLowerCase().trim()
      return new Promise((resolve) => {
        const tx = this.dictDb!.transaction([langCode], 'readonly')
        const store = tx.objectStore(langCode)
        const request = store.get(normalised)
        request.onsuccess = () => {
          const result = request.result as DictLookupResult | undefined
          resolve(result || null)
        }
        request.onerror = () => resolve(null)
      })
    } catch (err) {
      console.warn('storagemanager: dictLookup failed:', (err as Error).message)
      return null
    }
  }

  async dictGetDownloadedLanguages(): Promise<string[]> {
    try {
      if (!this.dictDb) await this.initDictionary()
      if (!this.dictDb!.objectStoreNames.contains('meta')) {
        console.log('[dict] no meta store found, returning empty list')
        return []
      }
      return new Promise((resolve) => {
        const tx = this.dictDb!.transaction(['meta'], 'readonly')
        const store = tx.objectStore('meta')
        const request = store.getAll()
        request.onsuccess = () => {
          const results = request.result as DictMetaEntry[]
          const downloaded = results
            .filter(m => m.key.startsWith('lang-') && m.downloaded)
            .map(m => m.key.replace('lang-', ''))
          console.log('[dict] downloaded languages:', downloaded)
          resolve(downloaded)
        }
        request.onerror = () => resolve([])
      })
    } catch (err) {
      console.warn('[dict] dictGetDownloadedLanguages failed:', (err as Error).message)
      return []
    }
  }

  async dictDeleteLanguage(langCode: string): Promise<void> {
    if (!this.dictDb) await this.initDictionary()
    return new Promise<void>((resolve, reject) => {
      const tx = this.dictDb!.transaction([langCode, 'meta'], 'readwrite')
      const langStore = tx.objectStore(langCode)
      const metaStore = tx.objectStore('meta')
      langStore.clear()
      metaStore.delete(`lang-${langCode}`)
      tx.oncomplete = async () => {
        await this._clearDownloadProgress(langCode)
        await this._markDownloading(langCode, false)
        resolve()
      }
      tx.onerror = () => reject(tx.error)
    })
  }
}

export const storageManager = new StorageManager()
