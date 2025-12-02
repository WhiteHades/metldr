import { UIService } from './UIService.js';
import { replyPanel } from './ReplyPanel.js';

export class EmailExtractor {
  constructor() {
    this.lastProcessedUrl = '';
    this.debounceTimer = null;
    this.observer = null;
    this.replyObserver = null;
    this.debounceDelay = 500;
    this.onEmailProcess = null;
    this.isProcessing = false;
    this.isRegenerating = false;
    this.processingTimeout = 30000;
    this.lastReplyPanelCheck = 0;
    this.replyCheckCooldown = 500;
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
    this.setupReplyObserver();
    this.setupUrlHooks();
    this.triggerInitialProcess();
    replyPanel.startWatching();
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

  setupReplyObserver() {
    const config = {
      childList: true,
      subtree: true
    };

    this.replyObserver = new MutationObserver((mutations) => {
      const now = Date.now();
      if (now - this.lastReplyPanelCheck < this.replyCheckCooldown) return;
      this.lastReplyPanelCheck = now;

      for (const mutation of mutations) {
        for (const node of mutation.addedNodes) {
          if (node.nodeType === Node.ELEMENT_NODE) {
            this._checkForReplyPanel(node);
          }
        }
      }
    });

    this.replyObserver.observe(document.body, config);
    console.log('[EmailExtractor] reply observer initialized');
    
    this._setupReplyButtonListener();
  }

  _setupReplyButtonListener() {
    document.addEventListener('click', (e) => {
      const replyTarget = e.target.closest('[data-tooltip*="Reply"], [aria-label*="Reply"], [data-tooltip*="reply"], .ams.bkH, .ams.bkI');
      const popOutTarget = e.target.closest('[data-tooltip*="Pop out"], [data-tooltip*="pop out"], [aria-label*="Pop out"], [aria-label*="pop out"], .aB2, [command="inNewWindow"]');
      
      if (replyTarget || popOutTarget) {
        console.log('[EmailExtractor] reply/popout button clicked:', replyTarget ? 'reply' : 'popout');
        setTimeout(() => this._findAndShowReplyPanel(), 400);
        setTimeout(() => this._findAndShowReplyPanel(), 800);
        setTimeout(() => this._findAndShowReplyPanel(), 1500);
        if (popOutTarget) {
          setTimeout(() => this._findAndShowReplyPanel(), 2000);
        }
      }
    }, true);

    document.addEventListener('keydown', (e) => {
      const active = document.activeElement;
      const isTyping = active && (
        active.tagName === 'INPUT' || 
        active.tagName === 'TEXTAREA' || 
        active.contentEditable === 'true' ||
        active.closest('[contenteditable="true"]')
      );
      
      if (isTyping) return;
      
      if (e.key === 'r' || e.key === 'a') {
        console.log('[EmailExtractor] reply keyboard shortcut detected:', e.key);
        setTimeout(() => this._findAndShowReplyPanel(), 400);
        setTimeout(() => this._findAndShowReplyPanel(), 800);
        setTimeout(() => this._findAndShowReplyPanel(), 1500);
      }
    }, true);
  }

  _findAndShowReplyPanel() {
    const editableSelectors = [
      '[g_editable="true"]',
      '.Am.Al.editable[contenteditable="true"]',
      '.Am.aiL.Al.editable[contenteditable="true"]',
      '[role="textbox"][contenteditable="true"][aria-label="Message Body"]',
    ];

    for (const selector of editableSelectors) {
      const editables = document.querySelectorAll(selector);
      for (const editable of editables) {
        if (!this._isVisible(editable)) continue;
        
        let container = editable.parentElement;
        let attempts = 0;
        while (container && attempts < 15) {
          if (container.querySelector('.bAK')) {
            const threadId = this.getCurrentThreadId();
            if (threadId) {
              console.log('[EmailExtractor] found compose area with toolbar, thread:', threadId);
              replyPanel.show(container, threadId);
              return;
            }
          }
          container = container.parentElement;
          attempts++;
        }
      }
    }
    console.log('[EmailExtractor] no visible compose area found');
  }

  _isVisible(el) {
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && 
           window.getComputedStyle(el).display !== 'none' &&
           window.getComputedStyle(el).visibility !== 'hidden';
  }

  _checkForReplyPanel(node) {
    const composeSelectors = [
      '.aSs',
      '.iN',
      '.aYF',
      '.aoP',
      '.M9',
      '.nH.Hd',
      '.AD',
      '.ip.iq',
      '.Am.Al.editable',
      '[role="dialog"]',
      '.I5',
    ];

    let composeArea = null;
    
    const editable = node.querySelector?.('[contenteditable="true"][role="textbox"]') ||
                    node.querySelector?.('[contenteditable="true"][g_editable="true"]') ||
                    node.querySelector?.('.Am.Al.editable');
    
    if (editable) {
      composeArea = editable.closest('.aSs') ||
                   editable.closest('.aoP') ||
                   editable.closest('.iN') || 
                   editable.closest('.aYF') || 
                   editable.closest('.M9') ||
                   editable.closest('.nH.Hd') ||
                   editable.closest('[role="dialog"]') ||
                   editable.parentElement?.parentElement;
    }
    
    if (!composeArea) {
      for (const selector of composeSelectors) {
        if (node.matches?.(selector)) {
          composeArea = node;
          break;
        }
        const found = node.querySelector?.(selector);
        if (found) {
          composeArea = found;
          break;
        }
      }
    }

    if (!composeArea) return;

    const hasEditable = composeArea.querySelector?.('[contenteditable="true"]') ||
                       composeArea.querySelector?.('.Am.Al.editable') ||
                       composeArea.querySelector?.('[g_editable="true"]') ||
                       editable;

    if (!hasEditable) return;

    const threadId = this.getCurrentThreadId();
    if (!threadId) {
      console.log('[EmailExtractor] reply detected but no thread ID available');
      return;
    }

    console.log('[EmailExtractor] reply panel detected for thread:', threadId, 'compose area:', composeArea.className);

    setTimeout(() => {
      replyPanel.show(composeArea, threadId);
    }, 300);
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
    if (this.replyObserver) {
      this.replyObserver.disconnect();
    }
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }
    replyPanel.hide();
  }
}

export const emailExtractor = new EmailExtractor();
