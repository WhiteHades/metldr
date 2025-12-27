import { SUPPORTED_LANGUAGES } from '../utils/storage'
import { storageService } from './StorageService'
import { logger } from './LoggerService'
import type { DictionaryEntry } from '../types'

const log = logger.createScoped('DictionaryService')
const DB_DICT = 'metldr-dictionary'
const DB_VERSION = 3

export class DictionaryService {
  private db: IDBDatabase | null = null
  private languageCache: string[] | null = null
  private languageCacheTime = 0
  private initPromise: Promise<void> | null = null

  async init(): Promise<void> {
    if (this.initPromise) return this.initPromise
    if (this.db) return

    this.initPromise = new Promise<void>((resolve, reject) => {
      log.log('opening db:', DB_DICT + ' v' + DB_VERSION)
      const req = indexedDB.open(DB_DICT, DB_VERSION)

      req.onerror = () => {
        log.error('open failed', req.error)
        this.initPromise = null
        reject(req.error)
      }

      req.onsuccess = () => {
        this.db = req.result
        const stores = Array.from(this.db.objectStoreNames)
        log.log('db opened, stores:', stores)
        this.initPromise = null
        resolve()
      }

      req.onupgradeneeded = (event) => {
        const target = event.target as IDBOpenDBRequest
        const db = target.result
        SUPPORTED_LANGUAGES.forEach(lang => {
          if (!db.objectStoreNames.contains(lang.code)) {
            db.createObjectStore(lang.code, { keyPath: 'word' })
          }
        })
        if (!db.objectStoreNames.contains('meta')) db.createObjectStore('meta', { keyPath: 'key' })
      }
    })

    return this.initPromise
  }

  async getSelectedLanguages(): Promise<string[]> {
    const now = Date.now()
    const CACHE_TTL = 30000

    if (this.languageCache && (now - this.languageCacheTime) < CACHE_TTL) {
      return this.languageCache
    }

    try {
      const stored = await storageService.get<string[] | Record<string, string>>('selectedLanguages', ['en'])
      let langs: string[]

      if (!stored) {
        langs = ['en']
      } else if (Array.isArray(stored)) {
        langs = stored
      } else if (typeof stored === 'object') {
        langs = Object.values(stored).filter((l): l is string => typeof l === 'string')
      } else {
        langs = [String(stored)]
      }

      if (!langs.length) langs = ['en']

      this.languageCache = langs
      this.languageCacheTime = now
      return langs
    } catch (err) {
      log.error('getSelectedLanguages failed', (err as Error).message)
      return ['en']
    }
  }

  async find(word: string, languages: string[] | null = null): Promise<DictionaryEntry | null> {
    if (!this.db) {
      try {
        await this.init()
      } catch (err) {
        log.error('init failed', (err as Error).message)
        return null
      }
    }

    const searchLangs = languages || (await this.getSelectedLanguages())
    const normalizedWord = word.toLowerCase().trim()
    
    log.debug('looking up: ' + normalizedWord + ' in langs:', searchLangs)

    for (const langCode of searchLangs) {
      try {
        if (!this.db!.objectStoreNames.contains(langCode)) {
          log.debug('store not found: ' + langCode)
          continue
        }

        const result = await new Promise<{ word: string; definition: string; pos?: string } | undefined>((resolve) => {
          const tx = this.db!.transaction([langCode], 'readonly')
          const store = tx.objectStore(langCode)
          const req = store.get(normalizedWord)

          req.onsuccess = () => resolve(req.result)
          req.onerror = () => {
            log.debug('get error: ' + req.error)
            resolve(undefined)
          }
        })

        if (result) {
          log.debug('found in ' + langCode + ': ' + result.word)
          return {
            definitions: [{
              definition: result.definition,
              partOfSpeech: result.pos || 'unknown',
              example: null,
              synonyms: []
            }],
            synonyms: [],
            language: langCode,
            source: 'local'
          }
        }
      } catch (err) {
        log.debug('lookup failed for ' + langCode + ': ' + (err as Error).message)
        continue
      }
    }

    log.debug('word not found in any lang')
    return null
  }

  async hasLanguage(langCode: string): Promise<boolean> {
    if (!this.db) {
      try {
        await this.init()
      } catch {
        return false
      }
    }

    return this.db!.objectStoreNames.contains(langCode)
  }

  async isLanguageDownloaded(langCode: string): Promise<boolean> {
    if (!this.db) {
      try {
        await this.init()
      } catch {
        return false
      }
    }
    if (!this.db!.objectStoreNames.contains('meta')) return false

    return new Promise((resolve) => {
      try {
        const tx = this.db!.transaction(['meta'], 'readonly')
        const store = tx.objectStore('meta')
        const req = store.get(`lang-${langCode}`)
        req.onsuccess = () => resolve(req.result?.downloaded === true)
        req.onerror = () => resolve(false)
      } catch {
        resolve(false)
      }
    })
  }

  async listAvailable(): Promise<string[]> {
    if (!this.db) {
      try {
        await this.init()
      } catch {
        return []
      }
    }

    const langs: string[] = []
    for (let i = 0; i < this.db!.objectStoreNames.length; i++) {
      langs.push(this.db!.objectStoreNames[i])
    }

    return langs
  }
}

export const dictionaryService = new DictionaryService()
