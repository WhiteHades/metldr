const DB_CACHE = 'metldr_cache';
const DB_VERSION = 3;
const STORE_SUMMARIES = 'summaries';
const STORE_PAGE_CACHE = 'page_cache';
const STORE_REPLY_SUGGESTIONS = 'reply_suggestions';

export class CacheService {
  constructor() {
    this.db = null;
  }

  async init() {
    if (this.db) return;

    return new Promise((resolve, reject) => {
      const req = indexedDB.open(DB_CACHE, DB_VERSION);

      req.onerror = () => {
        console.error('[CacheService] open failed:', req.error);
        reject(req.error);
      };

      req.onsuccess = () => {
        this.db = req.result;
        resolve();
      };

      req.onupgradeneeded = (event) => {
        const db = event.target.result;
        if (!db.objectStoreNames.contains(STORE_SUMMARIES)) {
          db.createObjectStore(STORE_SUMMARIES, { keyPath: 'emailId' });
        }
        if (!db.objectStoreNames.contains(STORE_PAGE_CACHE)) {
          db.createObjectStore(STORE_PAGE_CACHE, { keyPath: 'url' });
        }
        if (!db.objectStoreNames.contains(STORE_REPLY_SUGGESTIONS)) {
          db.createObjectStore(STORE_REPLY_SUGGESTIONS, { keyPath: 'emailId' });
        }
      };
    });
  }

  async getEmailSummary(emailId) {
    if (!this.db) await this.init();

    return new Promise((resolve) => {
      try {
        const tx = this.db.transaction([STORE_SUMMARIES], 'readonly');
        const store = tx.objectStore(STORE_SUMMARIES);
        const req = store.get(emailId);

        req.onsuccess = () => {
          const result = req.result;
          if (!result) {
            resolve(null);
            return;
          }

          // check ttl
          if (result.timestamp && result.ttl) {
            const now = Date.now();
            if (now > result.timestamp + result.ttl) {
              this.deleteEmailSummary(emailId).catch(() => {});
              resolve(null);
              return;
            }
          }

          resolve(result.summary);
        };

        req.onerror = () => resolve(null);
      } catch (err) {
        console.error('[CacheService.getEmailSummary]', err.message);
        resolve(null);
      }
    });
  }

  async setEmailSummary(emailId, summary, ttlMs = 7 * 24 * 60 * 60 * 1000) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_SUMMARIES], 'readwrite');
      const store = tx.objectStore(STORE_SUMMARIES);
      await store.put({
        emailId,
        summary,
        timestamp: Date.now(),
        ttl: ttlMs
      });
      
      chrome.runtime.sendMessage({ type: 'SUMMARY_ADDED', emailId }).catch(() => {});
    } catch (err) {
      console.error('[CacheService.setEmailSummary]', err.message);
    }
  }

  async deleteEmailSummary(emailId) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_SUMMARIES], 'readwrite');
      const store = tx.objectStore(STORE_SUMMARIES);
      await store.delete(emailId);
    } catch (err) {
      console.error('[CacheService.deleteEmailSummary]', err.message);
    }
  }

  async getPageSummary(url) {
    if (!this.db) await this.init();

    return new Promise((resolve) => {
      try {
        const tx = this.db.transaction([STORE_PAGE_CACHE], 'readonly');
        const store = tx.objectStore(STORE_PAGE_CACHE);
        const req = store.get(url);

        req.onsuccess = () => {
          const result = req.result;
          if (!result) {
            resolve(null);
            return;
          }

          // check ttl
          if (result.timestamp && result.ttl) {
            const now = Date.now();
            if (now > result.timestamp + result.ttl) {
              this.deletePageSummary(url).catch(() => {});
              resolve(null);
              return;
            }
          }

          resolve(result.summary);
        };

        req.onerror = () => resolve(null);
      } catch (err) {
        console.error('[CacheService.getPageSummary]', err.message);
        resolve(null);
      }
    });
  }

  async setPageSummary(url, summary, ttlSeconds = 3600) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_PAGE_CACHE], 'readwrite');
      const store = tx.objectStore(STORE_PAGE_CACHE);
      await store.put({
        url,
        summary,
        timestamp: Date.now(),
        ttl: ttlSeconds * 1000
      });
    } catch (err) {
      console.error('[CacheService.setPageSummary]', err.message);
    }
  }

  async deletePageSummary(url) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_PAGE_CACHE], 'readwrite');
      const store = tx.objectStore(STORE_PAGE_CACHE);
      await store.delete(url);
    } catch (err) {
      console.error('[CacheService.deletePageSummary]', err.message);
    }
  }

  async clearAll() {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_SUMMARIES, STORE_PAGE_CACHE, STORE_REPLY_SUGGESTIONS], 'readwrite');
      await tx.objectStore(STORE_SUMMARIES).clear();
      await tx.objectStore(STORE_PAGE_CACHE).clear();
      await tx.objectStore(STORE_REPLY_SUGGESTIONS).clear();
    } catch (err) {
      console.error('[CacheService.clearAll]', err.message);
    }
  }

  async getReplySuggestions(emailId) {
    if (!this.db) await this.init();

    return new Promise((resolve) => {
      try {
        const tx = this.db.transaction([STORE_REPLY_SUGGESTIONS], 'readonly');
        const store = tx.objectStore(STORE_REPLY_SUGGESTIONS);
        const req = store.get(emailId);

        req.onsuccess = () => {
          const result = req.result;
          if (!result) {
            resolve(null);
            return;
          }

          // check ttl (same as summaries: 7 days)
          if (result.timestamp && result.ttl) {
            const now = Date.now();
            if (now > result.timestamp + result.ttl) {
              this.deleteReplySuggestions(emailId).catch(() => {});
              resolve(null);
              return;
            }
          }

          resolve(result.suggestions);
        };

        req.onerror = () => resolve(null);
      } catch (err) {
        console.error('[CacheService.getReplySuggestions]', err.message);
        resolve(null);
      }
    });
  }

  async setReplySuggestions(emailId, suggestions, ttlMs = 7 * 24 * 60 * 60 * 1000) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_REPLY_SUGGESTIONS], 'readwrite');
      const store = tx.objectStore(STORE_REPLY_SUGGESTIONS);
      await store.put({
        emailId,
        suggestions,
        timestamp: Date.now(),
        ttl: ttlMs
      });
    } catch (err) {
      console.error('[CacheService.setReplySuggestions]', err.message);
    }
  }

  async deleteReplySuggestions(emailId) {
    if (!this.db) await this.init();

    try {
      const tx = this.db.transaction([STORE_REPLY_SUGGESTIONS], 'readwrite');
      const store = tx.objectStore(STORE_REPLY_SUGGESTIONS);
      await store.delete(emailId);
    } catch (err) {
      console.error('[CacheService.deleteReplySuggestions]', err.message);
    }
  }
}

export const cacheService = new CacheService();
