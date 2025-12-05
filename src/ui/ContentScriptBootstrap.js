import { UIService } from './UIService.js';
import { WordPopup } from './WordPopup.js';
import { emailExtractor } from './EmailExtractor.js';
import { ArticleExtractor } from './ArticleExtractor.js';

export class ContentScriptBootstrap {
  static async init(sdk = null) {
    const isGmail = window.location.hostname.includes('mail.google.com');

    UIService.init();
    await UIService.loadFromStorage();

    this.setupThemeListeners();
    this.setupMessageListeners();

    const popupManager = new WordPopup();
    document.addEventListener('mouseup', (e) => popupManager.handleTextSelection(e));

    if (isGmail && sdk) {
      emailExtractor.init(sdk, () => {
        // no callback, uses event handlers
      });
      console.log('metldr: gmail mode with inboxsdk');
    } else if (isGmail) {
      console.warn('metldr: gmail detected but no sdk available');
    } else {
      console.log('metldr: non-gmail mode, word popup only');
    }
  }

  static setupMessageListeners() {
    chrome.runtime.onMessage.addListener((msg, sender, respond) => {
      if (msg.type === 'EXTRACT_ARTICLE') {
        try {
          const extracted = ArticleExtractor.extract();
          respond({ success: true, data: extracted });
        } catch (err) {
          console.error('metldr: extraction failed:', err);
          respond({ success: false, error: err.message });
        }
        return true;
      }
      return false;
    });
  }

  static setupThemeListeners() {
    UIService.onChange((themeName, themeObj) => {
      console.log('metldr: theme changed to:', themeName);
      UIService.updatePopupTheme(document.querySelector('.metldr-inline-word-popup'));
      UIService.updateSummaryTheme();
    });

    chrome.storage.onChanged.addListener((changes, area) => {
      if (area === 'local' && changes.theme) {
        const themeName = changes.theme.newValue;
        UIService.setTheme(themeName);
      }
    });
  }
}
