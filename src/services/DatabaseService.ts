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
    if (existing) return existing

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
        resolve(request.result)
      }

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result

        for (const store of config.stores) {
          if (!db.objectStoreNames.contains(store.name)) {
            const objectStore = db.createObjectStore(store.name, { keyPath: store.keyPath })
            
            if (store.indexes) {
              for (const index of store.indexes) {
                objectStore.createIndex(index.name, index.keyPath, { unique: index.unique ?? false })
              }
            }
          }
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

  async getAll<T>(config: DatabaseConfig, storeName: string): Promise<T[]> {
    const db = await this.getDatabase(config)

    return new Promise((resolve) => {
      try {
        const tx = db.transaction([storeName], 'readonly')
        const store = tx.objectStore(storeName)
        const request = store.getAll()

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
    storeNames: string[],
    mode: TransactionMode,
    callback: (stores: Record<string, IDBObjectStore>) => Promise<T>
  ): Promise<T> {
    const db = await this.getDatabase(config)
    const tx = db.transaction(storeNames, mode)
    
    const stores: Record<string, IDBObjectStore> = {}
    for (const name of storeNames) {
      stores[name] = tx.objectStore(name)
    }

    return callback(stores)
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
    version: 5,
    stores: [
      { name: 'summaries', keyPath: 'emailId' },
      { name: 'page_cache', keyPath: 'url' },
      { name: 'reply_suggestions', keyPath: 'emailId' },
      { name: 'tab_sessions', keyPath: 'url' }
    ]
  },
  dictionary: {
    name: 'metldr-dictionary',
    version: 3,
    stores: [
      { name: 'words', keyPath: 'word' },
      { name: 'metadata', keyPath: 'lang' }
    ]
  }
} as const
