const DB_DICT = 'metldr-dictionary';
const DB_VERSION = 2;

export class DictionaryService {
  constructor() {
    this.db = null;
    this.languageCache = null;
    this.languageCacheTime = 0;
    this.initPromise = null;
  }

  async init() {
    if (this.initPromise) return this.initPromise;
    if (this.db) return;

    this.initPromise = new Promise((resolve, reject) => {
      console.log('[DictionaryService] opening db:', DB_DICT, 'v' + DB_VERSION);
      const req = indexedDB.open(DB_DICT, DB_VERSION);

      req.onerror = () => {
        console.error('[DictionaryService] open failed:', req.error);
        this.initPromise = null;
        reject(req.error);
      };

      req.onsuccess = () => {
        this.db = req.result;
        const stores = Array.from(this.db.objectStoreNames);
        console.log('[DictionaryService] db opened, stores:', stores);
        this.initPromise = null;
        resolve();
      };

      req.onupgradeneeded = (event) => {
        console.log('[DictionaryService] db upgrade needed');
        const db = event.target.result;
        if (!db.objectStoreNames.contains('meta')) {
          db.createObjectStore('meta', { keyPath: 'key' });
        }
      };
    });

    return this.initPromise;
  }

  async getSelectedLanguages() {
    const now = Date.now();
    const CACHE_TTL = 30000;

    if (this.languageCache && (now - this.languageCacheTime) < CACHE_TTL) {
      return this.languageCache;
    }

    try {
      const result = await chrome.storage.local.get(['selectedLanguages']);
      let langs = result.selectedLanguages || ['en'];

      if (!Array.isArray(langs)) {
        if (typeof langs === 'object' && langs !== null) {
          langs = Object.values(langs).filter(l => typeof l === 'string');
        } else {
          langs = [langs];
        }
      }

      if (!langs.length) langs = ['en'];

      this.languageCache = langs;
      this.languageCacheTime = now;
      return langs;
    } catch (err) {
      console.error('[DictionaryService.getSelectedLanguages]', err.message);
      return ['en'];
    }
  }

  async find(word, languages = null) {
    if (!this.db) {
      try {
        await this.init();
      } catch (err) {
        console.error('[DictionaryService.find] init failed:', err.message);
        return null;
      }
    }

    const searchLangs = languages || (await this.getSelectedLanguages());
    const normalizedWord = word.toLowerCase().trim();
    
    console.log('[DictionaryService.find] looking up:', normalizedWord, 'in langs:', searchLangs);

    for (const langCode of searchLangs) {
      try {
        if (!this.db.objectStoreNames.contains(langCode)) {
          console.log('[DictionaryService.find] store not found:', langCode);
          continue;
        }

        const result = await new Promise((resolve) => {
          const tx = this.db.transaction([langCode], 'readonly');
          const store = tx.objectStore(langCode);
          const req = store.get(normalizedWord);

          req.onsuccess = () => resolve(req.result);
          req.onerror = () => {
            console.log('[DictionaryService.find] get error:', req.error);
            resolve(null);
          };
        });

        if (result) {
          console.log('[DictionaryService.find] found in', langCode, ':', result.word);
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
          };
        }
      } catch (err) {
        console.log(`[DictionaryService.find] lookup failed for ${langCode}:`, err.message);
        continue;
      }
    }

    console.log('[DictionaryService.find] word not found in any lang');
    return null;
  }

  async hasLanguage(langCode) {
    if (!this.db) {
      try {
        await this.init();
      } catch (err) {
        return false;
      }
    }

    return this.db.objectStoreNames.contains(langCode);
  }

  async listAvailable() {
    if (!this.db) {
      try {
        await this.init();
      } catch (err) {
        return [];
      }
    }

    const langs = [];
    for (let i = 0; i < this.db.objectStoreNames.length; i++) {
      langs.push(this.db.objectStoreNames[i]);
    }

    return langs;
  }
}

export const dictionaryService = new DictionaryService();
