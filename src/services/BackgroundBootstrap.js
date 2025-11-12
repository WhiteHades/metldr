import { OllamaService } from './OllamaService.js';
import { cacheService } from './CacheService.js';
import { dictionaryService } from './DictionaryService.js';
import { WordService } from './WordService.js';
import { EmailService } from './EmailService.js';
import { PageService } from './PageService.js';

export class BackgroundBootstrap {
  static async init() {
    chrome.sidePanel
      .setPanelBehavior({ openPanelOnActionClick: true })
      .catch(err => console.error('[BackgroundBootstrap] side panel error:', err));

    await cacheService.init();
    await dictionaryService.init();

    chrome.runtime.onMessage.addListener((msg, sender, respond) => {
      if (msg.type === 'SUMMARIZE_EMAIL') {
        this._onEmailSummary(msg, respond);
        return true;
      }

      if (msg.type === 'PRE_SUMMARISE') {
        this._onPageSummary(msg, respond);
        return true;
      }

      if (msg.type === 'WORD_LOOKUP') {
        this._onWordLookup(msg, respond);
        return true;
      }

      if (msg.type === 'CHECK_OLLAMA_HEALTH') {
        this._onHealthCheck(respond);
        return true;
      }

      return false;
    });

    console.log('[BackgroundBootstrap] initialized');
  }

  static _onEmailSummary(msg, respond) {
    (async () => {
      try {
        const { emailContent, emailId, metadata, forceRegenerate } = msg;
        const summary = await EmailService.summarize(emailContent, emailId, metadata, forceRegenerate);
        respond({ summary });
      } catch (err) {
        console.error('[BackgroundBootstrap._onEmailSummary]', err.message);
        respond({
          summary: {
            summary: `error: ${err.message}`,
            action_items: [],
            dates: [],
            confidence: 'low'
          }
        });
      }
    })();
  }

  static _onPageSummary(msg, respond) {
    (async () => {
      try {
        const { content, pageType, metadata, url, forceRegenerate } = msg;
        const summary = await PageService.summarize(content, pageType, metadata, url, forceRegenerate);

        chrome.runtime.sendMessage({ type: 'PAGE_SUMMARY', summary }).catch(() => {});

        respond({ success: true, cached: false });
      } catch (err) {
        console.error('[BackgroundBootstrap._onPageSummary]', err.message);
        respond({ success: false, error: err.message });
      }
    })();
  }

  static _onWordLookup(msg, respond) {
    (async () => {
      try {
        const { word, context } = msg;

        const settings = await chrome.storage.local.get(['selectedLanguages', 'dictionarySource']);
        const languages = this._normalizeLanguages(settings.selectedLanguages || ['en']);

        const isEnglish = /^[a-zA-Z]+$/.test(word);
        let detectedLang = 'en';

        if (!isEnglish) {
          detectedLang = await WordService.detectLanguage(word, context?.fullSentence || '');
        }

        const priorityLangs = [detectedLang];
        for (const lang of languages) {
          if (lang !== detectedLang) priorityLangs.push(lang);
        }
        if (detectedLang !== 'en' && !languages.includes('en')) {
          priorityLangs.push('en');
        }

        const result = await WordService.lookup(word, { ...context, languages: priorityLangs });

        if (result) {
          respond({ success: true, result });
        } else {
          respond({ success: false, error: 'word not found' });
        }
      } catch (err) {
        console.error('[BackgroundBootstrap._onWordLookup]', err.message);
        respond({ success: false, error: err.message });
      }
    })();
  }

  static _onHealthCheck(respond) {
    (async () => {
      try {
        const { available, models } = await OllamaService.checkAvailable();
        respond({ success: true, connected: available, models });
      } catch (err) {
        console.error('[BackgroundBootstrap._onHealthCheck]', err.message);
        respond({ success: true, connected: false, models: [] });
      }
    })();
  }

  static _normalizeLanguages(langs) {
    if (!langs) return ['en'];

    if (!Array.isArray(langs)) {
      if (typeof langs === 'object' && langs !== null) {
        return Object.values(langs).filter(l => typeof l === 'string');
      }
      return [langs];
    }

    return langs.length > 0 ? langs : ['en'];
  }
}
