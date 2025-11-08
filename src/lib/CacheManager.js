// IndexedDB caching for ollama results, email summaries, dictionary lookups
// improves performance and reduces redundant API calls

export class CacheManager {
  constructor(dbName = 'metldr_cache', version = 1) {
    this.dbName = dbName;
    this.version = version;
    this.db = null;
  }

  async init() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.dbName, this.version);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.db = request.result;
        resolve();
      };

      request.onupgradeneeded = (event) => {
        const db = event.target.result;

        // stores
        if (!db.objectStoreNames.contains('ollama_cache')) {
          const ollamaStore = db.createObjectStore('ollama_cache', { keyPath: 'key' });
          ollamaStore.createIndex('timestamp', 'timestamp', { unique: false });
        }

        if (!db.objectStoreNames.contains('email_summaries')) {
          const emailStore = db.createObjectStore('email_summaries', { keyPath: 'emailId' });
          emailStore.createIndex('timestamp', 'timestamp', { unique: false });
        }

        if (!db.objectStoreNames.contains('word_definitions')) {
          db.createObjectStore('word_definitions', { keyPath: 'word' });
        }
      };
    });
  }

  // generic cache for ollama prompts
  async set(key, value, ttl = 7 * 24 * 60 * 60 * 1000) { // 7 days default
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['ollama_cache'], 'readwrite');
    const store = transaction.objectStore('ollama_cache');

    await store.put({
      key,
      value,
      timestamp: Date.now(),
      ttl
    });
  }

  async get(key) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['ollama_cache'], 'readonly');
    const store = transaction.objectStore('ollama_cache');

    const request = await store.get(key);
    if (!request) return null;

    const { value, timestamp, ttl } = request;
    const now = Date.now();

    // check ttl
    if (timestamp + ttl < now) {
      await this.delete(key);
      return null;
    }

    return value;
  }

  async delete(key) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['ollama_cache'], 'readwrite');
    const store = transaction.objectStore('ollama_cache');
    await store.delete(key);
  }

  // email-specific cache
  async setEmailSummary(emailId, summary, metadata = {}) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['email_summaries'], 'readwrite');
    const store = transaction.objectStore('email_summaries');

    await store.put({
      emailId,
      summary,
      timestamp: Date.now(),
      ...metadata
    });
  }

  async getEmailSummary(emailId) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['email_summaries'], 'readonly');
    const store = transaction.objectStore('email_summaries');

    const request = await store.get(emailId);
    return request ? { summary: request.summary, metadata: request } : null;
  }

  // dictionary cache
  async setWordDefinition(word, definition) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['word_definitions'], 'readwrite');
    const store = transaction.objectStore('word_definitions');

    await store.put({ word, definition });
  }

  async getWordDefinition(word) {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['word_definitions'], 'readonly');
    const store = transaction.objectStore('word_definitions');

    const request = await store.get(word);
    return request ? request.definition : null;
  }

  // clear all caches
  async clearAll() {
    if (!this.db) await this.init();

    const stores = ['ollama_cache', 'email_summaries', 'word_definitions'];

    for (const storeName of stores) {
      const transaction = this.db.transaction([storeName], 'readwrite');
      const store = transaction.objectStore(storeName);
      await store.clear();
    }
  }

  // cleanup expired entries
  async cleanup() {
    if (!this.db) await this.init();

    const transaction = this.db.transaction(['ollama_cache'], 'readwrite');
    const store = transaction.objectStore('ollama_cache');
    const index = store.index('timestamp');
    const request = index.openCursor();

    return new Promise((resolve) => {
      request.onsuccess = (event) => {
        const cursor = event.target.result;
        if (!cursor) {
          resolve();
          return;
        }

        const { timestamp, ttl } = cursor.value;
        if (Date.now() > timestamp + ttl) {
          cursor.delete();
        }

        cursor.continue();
      };
    });
  }
}

