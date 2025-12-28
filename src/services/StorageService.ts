type StorageKey = 
  | 'theme'
  | 'selectedModel'
  | 'preferredProvider'
  | 'wordPopupEnabled'
  | 'summaryPrefs'
  | 'selectedLanguages'
  | 'dictionarySource'
  | 'dictDownloadProgress'
  | 'downloadingLanguages'
  | 'fontSize'

class StorageServiceClass {
  private cache: Map<string, { value: unknown; timestamp: number }> = new Map()
  private readonly CACHE_TTL = 5000

  async get<T>(key: StorageKey, defaultVal: T): Promise<T> {
    const cached = this.cache.get(key)
    if (cached && Date.now() - cached.timestamp < this.CACHE_TTL) {
      return cached.value as T
    }

    try {
      const result = await chrome.storage.local.get([key])
      const value = result[key] !== undefined ? result[key] as T : defaultVal
      this.cache.set(key, { value, timestamp: Date.now() })
      return value
    } catch {
      return defaultVal
    }
  }

  async set<T>(key: StorageKey, value: T): Promise<void> {
    try {
      await chrome.storage.local.set({ [key]: value })
      this.cache.set(key, { value, timestamp: Date.now() })
    } catch (err) {
      console.error('[StorageService] set failed:', key, err)
    }
  }

  async getMultiple<T extends Record<string, unknown>>(keys: StorageKey[]): Promise<Partial<T>> {
    try {
      const result = await chrome.storage.local.get(keys)
      for (const key of keys) {
        if (result[key] !== undefined) {
          this.cache.set(key, { value: result[key], timestamp: Date.now() })
        }
      }
      return result as Partial<T>
    } catch {
      return {}
    }
  }

  async setMultiple(items: Partial<Record<StorageKey, unknown>>): Promise<void> {
    try {
      await chrome.storage.local.set(items)
      for (const [key, value] of Object.entries(items)) {
        this.cache.set(key, { value, timestamp: Date.now() })
      }
    } catch (err) {
      console.error('[StorageService] setMultiple failed:', err)
    }
  }

  async remove(key: StorageKey): Promise<void> {
    try {
      await chrome.storage.local.remove(key)
      this.cache.delete(key)
    } catch (err) {
      console.error('[StorageService] remove failed:', key, err)
    }
  }

  clearCache(): void {
    this.cache.clear()
  }

  onChange(callback: (changes: Record<string, chrome.storage.StorageChange>) => void): () => void {
    const listener = (changes: Record<string, chrome.storage.StorageChange>, area: string) => {
      if (area === 'local') {
        for (const key of Object.keys(changes)) {
          this.cache.delete(key)
        }
        callback(changes)
      }
    }
    chrome.storage.onChanged.addListener(listener)
    return () => chrome.storage.onChanged.removeListener(listener)
  }
}

export const storageService = new StorageServiceClass()
