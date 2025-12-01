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
    const hash = window.location.hash;
    
    // pattern #label/THREAD_ID or #inbox/THREAD_ID or #sent/THREAD_ID
    // typically 16+ chars alphanumeric
    const hashMatch = hash.match(/#[^/]+\/([A-Za-z0-9_-]{16,})$/);
    if (hashMatch) {
      return hashMatch[1];
    }

    const url = new URL(window.location.href);
    const queryId = url.searchParams.get('msgid') || url.searchParams.get('tid');
    if (queryId) return queryId;

    const threadEl = document.querySelector('[data-thread-perm-id]');
    if (threadEl) {
      return threadEl.getAttribute('data-thread-perm-id');
    }

    return null;
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
    console.log('metldr: processCurrentEmail called');
    
    if (this.isProcessing) {
      console.log('metldr: already processing, skipping');
      return;
    }

    const threadId = this.getCurrentThreadId();
    console.log('metldr: threadId =', threadId);
    if (!threadId) {
      console.log('metldr: no threadId found, checking URL:', window.location.href);
      return;
    }

    const existingSummary = document.querySelector('.metldr-summary');
    const existingLoading = document.querySelector('.metldr-loading');
    if (existingSummary || existingLoading) {
      console.log('metldr: summary/loading already exists, skipping');
      return;
    }

    const emailContainer = this.findEmailContainer();
    console.log('metldr: emailContainer =', emailContainer);
    if (!emailContainer) {
      console.log('metldr: no email container found');
      return;
    }

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
    console.log('metldr: processEmailThread started');
    try {
      const metadata = this.extractMetadata(threadElement);
      console.log('metldr: metadata =', metadata);
      
      const emailText = this.extractText(threadElement);
      console.log('metldr: emailText length =', emailText?.length || 0);

      if (!emailText || emailText.length < 50) {
        console.log('metldr: email text too short, skipping');
        return;
      }

      const threadId = this.getCurrentThreadId();

      let loadingDiv = null;
      const loadingTimer = setTimeout(() => {
        loadingDiv = UIService.createLoadingIndicator(threadElement);
        const emailHeader = this.findEmailHeader();
        UIService.injectLoading(emailHeader, loadingDiv);
      }, 100);

      console.log('metldr: requesting summary from background...');
      const summary = await this.getSummaryFromBackground(emailText, threadId, metadata);
      console.log('metldr: summary response =', summary);

      clearTimeout(loadingTimer);
      if (loadingDiv) loadingDiv.remove();

      if (summary) {
        console.log('metldr: creating summary card');
        const summaryCard = UIService.createSummaryCard(summary, threadId);
        const emailHeader = this.findEmailHeader();
        console.log('metldr: emailHeader for injection =', emailHeader);
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
    if (!chrome?.runtime?.sendMessage) {
      return null;
    }

    const maxRetries = 3;
    const delays = [0, 100, 200];

    for (let attempt = 0; attempt < maxRetries; attempt++) {
      if (delays[attempt] > 0) {
        await new Promise(r => setTimeout(r, delays[attempt]));
      }

      try {
        const response = await new Promise((resolve, reject) => {
          chrome.runtime.sendMessage({
            type: 'SUMMARIZE_EMAIL',
            emailContent: emailText,
            emailId: emailId,
            metadata: metadata,
            forceRegenerate: forceRegenerate
          }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message));
            } else {
              resolve(resp);
            }
          });
        });

        return response?.summary || null;
      } catch (err) {
        console.warn(`metldr: email summary attempt ${attempt + 1}/${maxRetries} failed:`, err.message);
        if (attempt === maxRetries - 1) {
          console.error('metldr: all email summary attempts failed');
          return null;
        }
      }
    }

    return null;
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
