import { UIService } from './UIService.js';

export class EmailExtractor {
  constructor() {
    this.lastProcessedUrl = '';
    this.debounceTimer = null;
    this.observer = null;
    this.debounceDelay = 500;
    this.onEmailProcess = null;
    this.isProcessing = false;
    this.isRegenerating = false;
    this.processingTimeout = 30000;
  }

  setProcessCallback(callback) {
    this.onEmailProcess = callback;
  }

  extractMetadata(threadElement = document) {
    const metadata = {
      date: null,
      sender: null,
      senderEmail: null,
      subject: null,
      to: null,
      timestamp: null
    };

    try {
      const dateElement = document.querySelector('span.g3[title]') || 
                         document.querySelector('.gH .gK span[title]') ||
                         document.querySelector('[data-tooltip][role="gridcell"] span[title]');
      if (dateElement) {
        metadata.date = dateElement.getAttribute('title') || dateElement.textContent?.trim();
        if (metadata.date) {
          const parsedDate = new Date(metadata.date);
          if (!isNaN(parsedDate.getTime())) {
            metadata.timestamp = parsedDate.toISOString();
          }
        }
      }

      const senderElement = document.querySelector('span.gD[email]') || 
                           document.querySelector('.gE.iv.gt span[email]') ||
                           document.querySelector('[email][data-hovercard-id]');
      if (senderElement) {
        metadata.senderEmail = senderElement.getAttribute('email');
        metadata.sender = senderElement.getAttribute('name') || senderElement.textContent?.trim();
      }

      const subjectElement = document.querySelector('h2.hP') || 
                            document.querySelector('.ha h2') ||
                            document.querySelector('[data-thread-perm-id] h2');
      if (subjectElement) {
        metadata.subject = subjectElement.textContent?.trim();
      }

      const toElement = document.querySelector('.gE.iv.gt .g2') ||
                       document.querySelector('[data-hovercard-id]');
      if (toElement) {
        metadata.to = toElement.textContent?.trim();
      }
    } catch (err) {
      console.error('metldr: email metadata extraction failed:', err);
    }

    return metadata;
  }

  extractText(threadElement = document) {
    const selectors = [
      '.ii.gt',
      '.a3s.aiL',
      '.ii',
      '[dir="ltr"]',
      '.gmail_signature'
    ];

    let fullText = '';
    for (const selector of selectors) {
      const elements = document.querySelectorAll(selector);
      if (elements.length > 0) {
        for (const element of elements) {
          const text = element.textContent?.trim();
          if (text && text.length > 20) {
            fullText += text + '\n\n';
          }
        }
      }
    }

    return fullText.split('\n').filter(line => line.trim().length > 0).join('\n');
  }

  findEmailHeader() {
    return document.querySelector('.gH') || 
           document.querySelector('.gE') ||
           document.querySelector('[data-thread-perm-id]') ||
           document.querySelector('div[role="main"]');
  }

  getCurrentThreadId() {
    const url = new URL(window.location.href);
    return url.searchParams.get('msgid') || 
           url.searchParams.get('tid') ||
           document.querySelector('[data-thread-perm-id]')?.getAttribute('data-thread-perm-id') ||
           null;
  }

  findEmailContainer() {
    return document.querySelector('.nH.if') ||
      document.querySelector('[data-thread-id]') ||
      document.querySelector('.gs') ||
      document.querySelector('div[role="main"]');
  }

  async init(onEmailProcess) {
    this.onEmailProcess = onEmailProcess;
    await this.waitForGmailUI();
    this.setupMutationObserver();
    this.setupUrlHooks();
    this.triggerInitialProcess();
  }

  async waitForGmailUI() {
    return new Promise((resolve) => {
      let attempts = 0;
      const maxAttempts = 50;
      const check = () => {
        const container = document.querySelector('div[role="main"]');
        if (container || attempts >= maxAttempts) {
          setTimeout(resolve, 500);
        } else {
          attempts++;
          setTimeout(check, 100);
        }
      };
      check();
    });
  }

  setupMutationObserver() {
    const config = {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['data-thread-id', 'data-thread-perm-id']
    };

    this.observer = new MutationObserver(() => {
      this.debounceProcess();
    });

    const mainContainer = document.querySelector('div[role="main"]');
    if (mainContainer) {
      this.observer.observe(mainContainer, config);
    }
  }

  setupUrlHooks() {
    const urlChangeHandler = () => {
      const currentUrl = window.location.href;
      if (currentUrl !== this.lastProcessedUrl) {
        this.lastProcessedUrl = currentUrl;
        this.debounceProcess();
      }
    };

    window.addEventListener('hashchange', urlChangeHandler);
    window.addEventListener('popstate', urlChangeHandler);
  }

  debounceProcess() {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => {
      if (this.onEmailProcess) {
        this.onEmailProcess();
      }
    }, this.debounceDelay);
  }

  triggerInitialProcess() {
    if (this.onEmailProcess) {
      setTimeout(() => this.onEmailProcess(), 1000);
    }
  }

  async processCurrentEmail() {
    if (this.isProcessing) return;

    const threadId = this.getCurrentThreadId();
    if (!threadId) return;

    const existingSummary = document.querySelector('.metldr-summary');
    const existingLoading = document.querySelector('.metldr-loading');
    if (existingSummary || existingLoading) return;

    const emailContainer = this.findEmailContainer();
    if (!emailContainer) return;

    this.isProcessing = true;

    const timeoutId = setTimeout(() => {
      this.isProcessing = false;
    }, this.processingTimeout);

    try {
      await this.processEmailThread(emailContainer);
    } finally {
      clearTimeout(timeoutId);
      this.isProcessing = false;
    }
  }

  async processEmailThread(threadElement) {
    try {
      const metadata = this.extractMetadata(threadElement);
      const emailText = this.extractText(threadElement);

      if (!emailText || emailText.length < 50) return;

      const threadId = this.getCurrentThreadId();

      let loadingDiv = null;
      const loadingTimer = setTimeout(() => {
        loadingDiv = UIService.createLoadingIndicator(threadElement);
        const emailHeader = this.findEmailHeader();
        UIService.injectLoading(emailHeader, loadingDiv);
      }, 100);

      const summary = await this.getSummaryFromBackground(emailText, threadId, metadata);

      clearTimeout(loadingTimer);
      if (loadingDiv) loadingDiv.remove();

      if (summary) {
        const summaryCard = UIService.createSummaryCard(summary, threadId);
        const emailHeader = this.findEmailHeader();
        UIService.injectSummary(emailHeader, summaryCard);

        this.attachRegenerateListener(summaryCard);
      }
    } catch (err) {
      console.error('metldr: email processing failed:', err);
    }
  }

  attachRegenerateListener(summaryCard) {
    const regenerateBtn = summaryCard.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        const threadId = regenerateBtn.getAttribute('data-thread-id');
        await this.regenerateSummary(threadId);
      });
    }
  }

  async getSummaryFromBackground(emailText, emailId, metadata = null, forceRegenerate = false) {
    return new Promise((resolve) => {
      if (!chrome?.runtime?.sendMessage) {
        resolve(null);
        return;
      }

      try {
        chrome.runtime.sendMessage({
          type: 'SUMMARIZE_EMAIL',
          emailContent: emailText,
          emailId: emailId,
          metadata: metadata,
          forceRegenerate: forceRegenerate
        }, (response) => {
          if (chrome.runtime.lastError) {
            resolve(null);
          } else {
            resolve(response?.summary || null);
          }
        });
      } catch (err) {
        console.error('metldr: message send failed:', err);
        resolve(null);
      }
    });
  }

  async regenerateSummary(threadId) {
    if (this.isRegenerating) return;

    this.isRegenerating = true;

    try {
      const existing = document.querySelector('.metldr-summary');
      if (existing) {
        existing.style.transition = 'opacity 0.2s cubic-bezier(0.4, 0, 1, 1), transform 0.2s cubic-bezier(0.4, 0, 1, 1)';
        existing.style.opacity = '0';
        existing.style.transform = 'scale(0.95)';
        await new Promise(resolve => setTimeout(resolve, 200));
        existing.remove();
      }

      const metadata = this.extractMetadata(document);
      const emailText = this.extractText(document);
      if (emailText && emailText.length > 50) {
        const emailContainer = this.findEmailContainer();

        if (emailContainer) {
          const loadingDiv = UIService.createLoadingIndicator(emailContainer);
          const emailHeader = this.findEmailHeader();
          UIService.injectLoading(emailHeader, loadingDiv);

          const summary = await this.getSummaryFromBackground(emailText, threadId, metadata, true);
          if (loadingDiv) loadingDiv.remove();

          if (summary) {
            const summaryCard = UIService.createSummaryCard(summary, threadId);
            UIService.injectSummary(emailHeader, summaryCard);
            this.attachRegenerateListener(summaryCard);
          }
        }
      }
    } finally {
      this.isRegenerating = false;
    }
  }

  destroy() {
    if (this.observer) {
      this.observer.disconnect();
    }
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }
  }
}

export const emailExtractor = new EmailExtractor();
