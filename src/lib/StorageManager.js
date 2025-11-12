const CACHE_DB_NAME = 'metldr_cache';
const CACHE_DB_VERSION = 1;
const DICT_DB_NAME = 'metldr-dictionary';
const DICT_DB_VERSION = 1;
const DICT_BASE_URL = 'https://media.githubusercontent.com/media/WhiteHades/wikitionary-dictionary-json/master/dist';

export const SUPPORTED_LANGUAGES = [
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
];

export class StorageManager {
  constructor() {
    this.cacheDb = null;
    this.dictDb = null;
  }

  async initCache() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(CACHE_DB_NAME, CACHE_DB_VERSION);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.cacheDb = request.result;
        resolve();
      };
      request.onupgradeneeded = (event) => {
        const db = event.target.result;
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

  async cacheSet(key, value, ttl = 7 * 24 * 60 * 60 * 1000) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['ollama_cache'], 'readwrite');
    const store = transaction.objectStore('ollama_cache');
    await store.put({ key, value, timestamp: Date.now(), ttl });
  }

  async cacheGet(key) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['ollama_cache'], 'readonly');
    const store = transaction.objectStore('ollama_cache');
    const request = await store.get(key);
    if (!request) return null;
    const { value, timestamp, ttl } = request;
    if (Date.now() > timestamp + ttl) {
      await this.cacheDelete(key);
      return null;
    }
    return value;
  }

  async cacheDelete(key) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['ollama_cache'], 'readwrite');
    const store = transaction.objectStore('ollama_cache');
    await store.delete(key);
  }

  async emailSummarySet(emailId, summary, metadata = {}) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['email_summaries'], 'readwrite');
    const store = transaction.objectStore('email_summaries');
    await store.put({ emailId, summary, timestamp: Date.now(), ...metadata });
  }

  async emailSummaryGet(emailId) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['email_summaries'], 'readonly');
    const store = transaction.objectStore('email_summaries');
    const request = await store.get(emailId);
    return request ? { summary: request.summary, metadata: request } : null;
  }

  async wordDefinitionSet(word, definition) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['word_definitions'], 'readwrite');
    const store = transaction.objectStore('word_definitions');
    await store.put({ word, definition });
  }

  async wordDefinitionGet(word) {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['word_definitions'], 'readonly');
    const store = transaction.objectStore('word_definitions');
    const request = await store.get(word);
    return request ? request.definition : null;
  }

  async cacheClearAll() {
    if (!this.cacheDb) await this.initCache();
    const stores = ['ollama_cache', 'email_summaries', 'word_definitions'];
    for (const storeName of stores) {
      const transaction = this.cacheDb.transaction([storeName], 'readwrite');
      const store = transaction.objectStore(storeName);
      await store.clear();
    }
  }

  async cacheCleanup() {
    if (!this.cacheDb) await this.initCache();
    const transaction = this.cacheDb.transaction(['ollama_cache'], 'readwrite');
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

  async initDictionary() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(DICT_DB_NAME, DICT_DB_VERSION);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.dictDb = request.result;
        resolve(this.dictDb);
      };
      request.onupgradeneeded = (event) => {
        const db = event.target.result;
        SUPPORTED_LANGUAGES.forEach(lang => {
          if (!db.objectStoreNames.contains(lang.code)) {
            db.createObjectStore(lang.code, { keyPath: 'word' });
          }
        });
        if (!db.objectStoreNames.contains('meta')) {
          db.createObjectStore('meta', { keyPath: 'key' });
        }
      };
    });
  }

  async dictIsLanguageDownloaded(langCode) {
    if (!this.dictDb) await this.initDictionary();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction(['meta'], 'readonly');
      const store = tx.objectStore('meta');
      const request = store.get(`lang-${langCode}`);
      request.onsuccess = () => {
        resolve(request.result?.downloaded === true);
      };
      request.onerror = () => reject(request.error);
    });
  }

  async dictMarkLanguageDownloaded(langCode) {
    if (!this.dictDb) await this.initDictionary();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction(['meta'], 'readwrite');
      const store = tx.objectStore('meta');
      const request = store.put({
        key: `lang-${langCode}`,
        downloaded: true,
        timestamp: Date.now()
      });
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  }

  async dictDownloadLanguage(langCode, onProgress) {
    const lang = SUPPORTED_LANGUAGES.find(l => l.code === langCode);
    if (!lang) throw new Error(`language ${langCode} not supported`);
    if (!this.dictDb) await this.initDictionary();

    const letters = 'abcdefghijklmnopqrstuvwxyz'.split('');
    let totalEntries = 0;

    for (let i = 0; i < letters.length; i++) {
      const letter = letters[i];
      const url = `${lang.url}/${letter}.json`;
      try {
        const response = await fetch(url);
        if (!response.ok) {
          console.warn(`failed to fetch ${url}: ${response.status}`);
          continue;
        }
        const entries = await response.json();
        await this.dictBatchInsert(langCode, entries);
        totalEntries += entries.length;
        if (onProgress) {
          onProgress({
            letter,
            progress: ((i + 1) / letters.length) * 100,
            entriesProcessed: totalEntries
          });
        }
      } catch (err) {
        console.error(`error downloading ${letter}.json for ${lang.name}:`, err);
      }
    }

    await this.dictMarkLanguageDownloaded(langCode);
    return totalEntries;
  }

  async dictBatchInsert(langCode, entries) {
    if (!this.dictDb) await this.initDictionary();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction([langCode], 'readwrite');
      const store = tx.objectStore(langCode);
      entries.forEach(entry => {
        store.put({
          word: entry.word.toLowerCase(),
          pos: entry.pos,
          definition: entry.definition
        });
      });
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  async dictLookupFromAPI(word, langCode = 'en') {
    try {
      const url = `https://api.dictionaryapi.dev/api/v2/entries/${langCode}/${word}`;
      const response = await fetch(url);
      if (!response.ok) return null;
      const data = await response.json();
      if (!data || !data[0] || !data[0].meanings) return null;
      const firstMeaning = data[0].meanings[0];
      const firstDef = firstMeaning.definitions[0];
      return {
        word: word.toLowerCase(),
        pos: firstMeaning.partOfSpeech || 'unknown',
        definition: firstDef.definition || '',
        source: 'api'
      };
    } catch (error) {
      console.error('storage manager: API lookup failed:', error);
      return null;
    }
  }

  async dictLookup(word, langCode = 'en') {
    if (!this.dictDb) await this.initDictionary();
    const normalised = word.toLowerCase().trim();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction([langCode], 'readonly');
      const store = tx.objectStore(langCode);
      const request = store.get(normalised);
      request.onsuccess = () => {
        resolve(request.result || null);
      };
      request.onerror = () => reject(request.error);
    });
  }

  async dictGetDownloadedLanguages() {
    if (!this.dictDb) await this.initDictionary();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction(['meta'], 'readonly');
      const store = tx.objectStore('meta');
      const request = store.getAll();
      request.onsuccess = () => {
        const downloaded = request.result
          .filter(m => m.key.startsWith('lang-') && m.downloaded)
          .map(m => m.key.replace('lang-', ''));
        resolve(downloaded);
      };
      request.onerror = () => reject(request.error);
    });
  }

  async dictDeleteLanguage(langCode) {
    if (!this.dictDb) await this.initDictionary();
    return new Promise((resolve, reject) => {
      const tx = this.dictDb.transaction([langCode, 'meta'], 'readwrite');
      const langStore = tx.objectStore(langCode);
      const metaStore = tx.objectStore('meta');
      langStore.clear();
      metaStore.delete(`lang-${langCode}`);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
}

export const storageManager = new StorageManager();
