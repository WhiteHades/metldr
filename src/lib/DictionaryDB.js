// indexeddb manager for offline dictionaries
// stores word definitions from wiktionary in perlanguage object stores

const DB_NAME = 'metldr-dictionary';
const DB_VERSION = 1;
const BASE_URL = 'https://media.githubusercontent.com/media/WhiteHades/wikitionary-dictionary-json/master/dist';

export const SUPPORTED_LANGUAGES = [
  { code: 'en', name: 'English', url: `${BASE_URL}/English` },
  { code: 'es', name: 'Spanish', url: `${BASE_URL}/Spanish` },
  { code: 'fr', name: 'French', url: `${BASE_URL}/French` },
  { code: 'de', name: 'German', url: `${BASE_URL}/German` },
  { code: 'it', name: 'Italian', url: `${BASE_URL}/Italian` },
  { code: 'pt', name: 'Portuguese', url: `${BASE_URL}/Portuguese` },
  { code: 'nl', name: 'Dutch', url: `${BASE_URL}/Dutch` },
  { code: 'sv', name: 'Swedish', url: `${BASE_URL}/Swedish` },
  { code: 'pl', name: 'Polish', url: `${BASE_URL}/Polish` },
  { code: 'ro', name: 'Romanian', url: `${BASE_URL}/Romanian` },
  { code: 'cs', name: 'Czech', url: `${BASE_URL}/Czech` },
  { code: 'fi', name: 'Finnish', url: `${BASE_URL}/Finnish` },
  { code: 'da', name: 'Danish', url: `${BASE_URL}/Danish` },
  { code: 'no', name: 'Norwegian', url: `${BASE_URL}/Norwegian` },
  { code: 'nb', name: 'Norwegian (Bokmål)', url: `${BASE_URL}/Norwegian%20(Bokm%C3%A5l)` },
  { code: 'nn', name: 'Norwegian (Nynorsk)', url: `${BASE_URL}/Norwegian%20(Nynorsk)` },
  { code: 'id', name: 'Indonesian', url: `${BASE_URL}/Indonesian` },
  { code: 'ms', name: 'Malay', url: `${BASE_URL}/Malay` },
  { code: 'tr', name: 'Turkish', url: `${BASE_URL}/Turkish` },
  { code: 'vi', name: 'Vietnamese', url: `${BASE_URL}/Vietnamese` },
  { code: 'lt', name: 'Lithuanian', url: `${BASE_URL}/Lithuanian` },
  { code: 'sk', name: 'Slovak', url: `${BASE_URL}/Slovak` }
];

class DictionaryDB {
  constructor() {
    this.db = null;
  }

  async init() {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.db = request.result;
        resolve(this.db);
      };

      request.onupgradeneeded = (event) => {
        const db = event.target.result;
        
        // create object store for each language
        SUPPORTED_LANGUAGES.forEach(lang => {
          if (!db.objectStoreNames.contains(lang.code)) {
            // word is the key
            db.createObjectStore(lang.code, { keyPath: 'word' });
          }
        });

        // metadata store for tracking downloads
        if (!db.objectStoreNames.contains('meta')) {
          db.createObjectStore('meta', { keyPath: 'key' });
        }
      };
    });
  }

  // check if a language is fully downloaded
  async isLanguageDownloaded(langCode) {
    if (!this.db) await this.init();
    
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(['meta'], 'readonly');
      const store = tx.objectStore('meta');
      const request = store.get(`lang-${langCode}`);
      
      request.onsuccess = () => {
        resolve(request.result?.downloaded === true);
      };
      request.onerror = () => reject(request.error);
    });
  }

  // mark language as downloaded
  async markLanguageDownloaded(langCode) {
    if (!this.db) await this.init();
    
    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(['meta'], 'readwrite');
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

  // download and store a language dictionary
  async downloadLanguage(langCode, onProgress) {
    const lang = SUPPORTED_LANGUAGES.find(l => l.code === langCode);
    if (!lang) throw new Error(`language ${langCode} not supported`);

    if (!this.db) await this.init();

    // fetch all 26 letter files (a.json through z.json)
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
        
        // batch insert into indexeddb
        await this.batchInsert(langCode, entries);
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

    await this.markLanguageDownloaded(langCode);
    return totalEntries;
  }

  // batch insert entries into language store
  async batchInsert(langCode, entries) {
    if (!this.db) await this.init();

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([langCode], 'readwrite');
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

  // free dictionary api
  async lookupFromAPI(word, langCode = 'en') {
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
      console.error('[DictionaryDB] API lookup failed:', error);
      return null;
    }
  }

  // lookup a word in a specific language
  async lookup(word, langCode = 'en') {
    if (!this.db) await this.init();

    const normalised = word.toLowerCase().trim();

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([langCode], 'readonly');
      const store = tx.objectStore(langCode);
      const request = store.get(normalised);

      request.onsuccess = () => {
        resolve(request.result || null);
      };
      request.onerror = () => reject(request.error);
    });
  }

  // get list of downloaded languages
  async getDownloadedLanguages() {
    if (!this.db) await this.init();

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction(['meta'], 'readonly');
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

  // delete a language dictionary
  async deleteLanguage(langCode) {
    if (!this.db) await this.init();

    return new Promise((resolve, reject) => {
      const tx = this.db.transaction([langCode, 'meta'], 'readwrite');
      const langStore = tx.objectStore(langCode);
      const metaStore = tx.objectStore('meta');

      // clear all entries
      langStore.clear();
      metaStore.delete(`lang-${langCode}`);

      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
}

export const dictionaryDB = new DictionaryDB();