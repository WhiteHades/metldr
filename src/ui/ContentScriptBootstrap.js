import { UIService } from './UIService.js';
import { WordPopup } from './WordPopup.js';
import { emailExtractor } from './EmailExtractor.js';
import { pageMonitor } from './PageMonitor.js';

export class ContentScriptBootstrap {
  static async init() {
    const isGmail = window.location.hostname.includes('mail.google.com');

    UIService.init();
    await UIService.loadFromStorage();

    this.setupThemeListeners();

    const popupManager = new WordPopup();

    if (isGmail) {
      emailExtractor.setProcessCallback(() => emailExtractor.processCurrentEmail());
      await emailExtractor.init(() => emailExtractor.processCurrentEmail());
    }

    document.addEventListener('mouseup', (e) => popupManager.handleTextSelection(e));

    if (!isGmail) {
      pageMonitor.setPreSummarizeCallback((preSumData) => {
        chrome.runtime.sendMessage({
          type: 'PRE_SUMMARISE',
          ...preSumData
        }).catch(err => {
          console.error('metldr: failed to queue pre-summarization:', err);
        });
      });
      pageMonitor.startDwellMonitoring();
    }
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
