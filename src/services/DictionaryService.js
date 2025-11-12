const DB_DICT = 'metldr-dictionary';
const DB_VERSION = 1;

export class DictionaryService {
  constructor() {
    this.db = null;
    this.languageCache = null;
    this.languageCacheTime = 0;
    const CACHE_TTL_MS = 30000;
  }

  async init() {
    if (this.db) return;

    return new Promise((resolve, reject) => {
      const req = indexedDB.open(DB_DICT, DB_VERSION);

      req.onerror = () => {
        console.error('[DictionaryService] open failed:', req.error);
        reject(req.error);
      };

      req.onsuccess = () => {
        this.db = req.result;
        resolve();
      };
    });
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
        return null;
      }
    }

    const searchLangs = languages || (await this.getSelectedLanguages());

    for (const langCode of searchLangs) {
      try {
        if (!this.db.objectStoreNames.contains(langCode)) continue;

        const result = await new Promise((resolve) => {
          const tx = this.db.transaction([langCode], 'readonly');
          const store = tx.objectStore(langCode);
          const req = store.get(word);

          req.onsuccess = () => resolve(req.result);
          req.onerror = () => resolve(null);
        });

        if (result) {
          return {
            word,
            definition: result.definition,
            partOfSpeech: result.pos,
            synonyms: result.synonyms || [],
            language: langCode,
            source: 'local'
          };
        }
      } catch (err) {
        console.log(`[DictionaryService] lookup failed for ${langCode}:`, err.message);
        continue;
      }
    }

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
