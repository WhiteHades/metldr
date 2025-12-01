import { UIService } from './UIService.js';
import { WordPopup } from './WordPopup.js';
import { emailExtractor } from './EmailExtractor.js';
import { ArticleExtractor } from './ArticleExtractor.js';

export class ContentScriptBootstrap {
  static async init() {
    const isGmail = window.location.hostname.includes('mail.google.com');

    UIService.init();
    await UIService.loadFromStorage();

    this.setupThemeListeners();
    this.setupMessageListeners();

    const popupManager = new WordPopup();

    if (isGmail) {
      emailExtractor.setProcessCallback(() => emailExtractor.processCurrentEmail());
      await emailExtractor.init(() => emailExtractor.processCurrentEmail());
    }

    document.addEventListener('mouseup', (e) => popupManager.handleTextSelection(e));
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
