import { CACHE_STORE_CONFIGS, migrateCache } from './cacheRecords'

interface StoreConfig {
  readonly name: string
  readonly keyPath: string
  readonly indexes?: ReadonlyArray<{ name: string; keyPath: string; unique?: boolean }>
}

interface DatabaseConfig {
  name: string
  version: number
  stores: readonly StoreConfig[] | StoreConfig[]
}

type TransactionMode = 'readonly' | 'readwrite'

class DatabaseServiceClass {
  private databases: Map<string, IDBDatabase> = new Map()
  private initPromises: Map<string, Promise<IDBDatabase>> = new Map()

  async getDatabase(config: DatabaseConfig): Promise<IDBDatabase> {
    const existing = this.databases.get(config.name)
    if (existing?.version === config.version) return existing
    if (existing) {
      existing.close()
      this.databases.delete(config.name)
    }

    const pending = this.initPromises.get(config.name)
    if (pending) return pending

    const promise = this.openDatabase(config)
    this.initPromises.set(config.name, promise)

    try {
      const db = await promise
      this.databases.set(config.name, db)
      return db
    } finally {
      this.initPromises.delete(config.name)
    }
  }

  private openDatabase(config: DatabaseConfig): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(config.name, config.version)

      request.onerror = () => {
        console.error('[DatabaseService] open failed:', config.name, request.error)
        reject(request.error)
      }

      request.onsuccess = () => {
        const db = request.result
        db.onversionchange = () => {
          db.close()
          if (this.databases.get(config.name) === db) this.databases.delete(config.name)
        }
        resolve(db)
      }

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result

        for (const store of config.stores) {
          const objectStore = db.objectStoreNames.contains(store.name)
            ? request.transaction!.objectStore(store.name)
            : db.createObjectStore(store.name, { keyPath: store.keyPath })
          if (store.indexes) {
            for (const index of store.indexes) {
              if (!objectStore.indexNames.contains(index.name)) {
                objectStore.createIndex(index.name, index.keyPath, { unique: index.unique ?? false })
              }
            }
          }
        }
        if (config.name === DB_CONFIGS.cache.name && event.oldVersion < 8) {
          migrateCache(request.transaction!)
        }
      }
    })
  }

  async get<T>(config: DatabaseConfig, storeName: string, key: string): Promise<T | null> {
    const db = await this.getDatabase(config)
    
    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readonly')
        const store = tx.objectStore(storeName)
        const request = store.get(key)

        request.onsuccess = () => resolve(request.result ?? null)
        request.onerror = () => resolve(null)
      } catch {
        resolve(null)
      }
    })
  }

  async put<T>(config: DatabaseConfig, storeName: string, value: T): Promise<void> {
    const db = await this.getDatabase(config)

    return new Promise((resolve, reject) => {
      try {
        const tx = db.transaction([storeName], 'readwrite')
        const store = tx.objectStore(storeName)
        const request = store.put(value)

        request.onsuccess = () => resolve()
        request.onerror = () => reject(request.error)
      } catch (err) {
        reject(err)
      }
    })
  }

  // batch put
  async putBatch<T>(config: DatabaseConfig, storeName: string, items: T[], chunkSize = 500): Promise<void> {
    const db = await this.getDatabase(config)
    
    for (let i = 0; i < items.length; i += chunkSize) {
      const chunk = items.slice(i, i + chunkSize)
      
      await new Promise<void>((resolve, reject) => {
        const tx = db.transaction([storeName], 'readwrite')
        const store = tx.objectStore(storeName)
        
        for (const item of chunk) {
          store.put(item)
        }
        
        tx.oncomplete = () => resolve()
        tx.onerror = () => reject(tx.error)
      })
    }
    
    console.log(`[DatabaseService] putBatch: ${items.length} items to ${storeName}`)
  }

  async delete(config: DatabaseConfig, storeName: string, key: string): Promise<void> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readwrite')
        const store = tx.objectStore(storeName)
        const request = store.delete(key)

        request.onsuccess = () => resolve()
        request.onerror = () => resolve()
      } catch {
        resolve()
      }
    })
  }

  async getAll<T>(config: DatabaseConfig, storeName: string, query?: IDBValidKey | IDBKeyRange): Promise<T[]> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readonly')
        const store = tx.objectStore(storeName)
        const request = store.getAll(query)

        request.onsuccess = () => resolve(request.result ?? [])
        request.onerror = () => resolve([])
      } catch {
        resolve([])
      }
    })
  }

  async clear(config: DatabaseConfig, storeName: string): Promise<void> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readwrite')
        const store = tx.objectStore(storeName)
        const request = store.clear()

        request.onsuccess = () => resolve()
        request.onerror = () => resolve()
      } catch {
        resolve()
      }
    })
  }

  async getAllKeys<K extends IDBValidKey>(config: DatabaseConfig, storeName: string, query?: IDBValidKey | IDBKeyRange, indexName?: string): Promise<K[]> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readonly')
        const store = tx.objectStore(storeName)
        const request = (indexName ? store.index(indexName) : store).getAllKeys(query)
        request.onsuccess = () => resolve(request.result as K[])
        request.onerror = () => resolve([])
      } catch {
        resolve([])
      }
    })
  }

  async count(config: DatabaseConfig, storeName: string): Promise<number> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readonly')
        const store = tx.objectStore(storeName)
        const request = store.count()

        request.onsuccess = () => resolve(request.result)
        request.onerror = () => resolve(0)
      } catch {
        resolve(0)
      }
    })
  }

  async transaction<T>(
    config: DatabaseConfig,
    storeNames: readonly string[],
    mode: TransactionMode,
    callback: (transaction: IDBTransaction) => T | Promise<T>
  ): Promise<T> {
    const db = await this.getDatabase(config)
    const tx = db.transaction([...storeNames], mode)
    const complete = new Promise<void>((resolve, reject) => {
      tx.oncomplete = () => resolve()
      tx.onabort = () => reject(tx.error ?? new Error('Database transaction aborted'))
    })
    try {
      const [value] = await Promise.all([callback(tx), complete])
      return value
    } catch (error) {
      try { tx.abort() } catch { /* The transaction may already have finished. */ }
      await complete.catch(() => {})
      throw error
    }
  }

  closeDatabase(name: string): void {
    const db = this.databases.get(name)
    if (db) {
      db.close()
      this.databases.delete(name)
    }
  }

  closeAll(): void {
    for (const [name, db] of this.databases) {
      db.close()
      this.databases.delete(name)
    }
  }
}

export const databaseService = new DatabaseServiceClass()

export const DB_CONFIGS = {
  cache: {
    name: 'metldr_cache',
    version: 8,
    stores: CACHE_STORE_CONFIGS
  },
  dictionary: {
    name: 'metldr-dictionary',
    version: 3,
    stores: [
      { name: 'words', keyPath: 'word' },
      { name: 'metadata', keyPath: 'lang' }
    ]
  },
  analytics: {
    name: 'metldr-analytics',
    version: 2,
    stores: [
      { name: 'daily_stats', keyPath: 'date', indexes: [{ name: 'timeInPanel', keyPath: 'timeInPanel' }] },
      { name: 'sessions', keyPath: 'id', indexes: [{ name: 'startTime', keyPath: 'startTime' }] },
      { name: 'events', keyPath: 'id', indexes: [{ name: 'timestamp', keyPath: 'timestamp' }] }
    ]
  }
} as const

