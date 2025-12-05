import { UIService } from './UIService.js';
import { replyPanel } from './ReplyPanel.js';

export class EmailExtractor {
  constructor() {
    this.sdk = null;
    this.onEmailProcess = null;
    this.isProcessing = false;
    this.isRegenerating = false;
    this.processingTimeout = 30000;
    this.processedThreads = new Set();
  }

  setProcessCallback(callback) {
    this.onEmailProcess = callback;
  }

  init(sdk, onEmailProcess) {
    this.sdk = sdk;
    this.onEmailProcess = onEmailProcess;

    if (!sdk) {
      console.warn('metldr: no sdk provided, email extraction disabled');
      return;
    }

    sdk.Conversations.registerThreadViewHandler((threadView) => {
      this.handleThreadView(threadView);
    });

    replyPanel.init(sdk);

    console.log('metldr: email extractor initialized with inboxsdk');
  }

  async handleThreadView(threadView) {
    let threadId = this.getThreadIdFromUrl();
    
    if (!threadId && threadView.getThreadIDAsync) {
      try {
        threadId = await threadView.getThreadIDAsync();
      } catch { // ignore
      }
    }

    if (!threadId) {
      console.log('metldr: no thread id available');
      return;
    }

    if (this.isProcessing) {
      console.log('metldr: already processing, skipping');
      return;
    }

  
    const existingSummary = document.querySelector('.metldr-summary');
    if (existingSummary) {
      const existingThreadId = existingSummary.getAttribute('data-metldr-thread');
      if (existingThreadId === threadId) {
        console.log('metldr: summary already exists for this thread');
        return;
      }
      existingSummary.remove();
    }

    this.isProcessing = true;

    const timeoutId = setTimeout(() => {
      this.isProcessing = false;
    }, this.processingTimeout);

    try {
      await this.processThread(threadView, threadId);
    } finally {
      clearTimeout(timeoutId);
      this.isProcessing = false;
    }
  }

  async processThread(threadView, threadId) {
    console.log('metldr: processing thread:', threadId);

    const subject = threadView.getSubject();
    const messages = threadView.getMessageViewsAll();

    if (!messages || messages.length === 0) {
      console.log('metldr: no messages in thread');
      return;
    }

    let fullText = '';
    const participants = new Set();
    let latestDate = null;

    for (const msgView of messages) {
      try {
        const sender = msgView.getSender();
        const bodyElement = msgView.getBodyElement();
        const body = bodyElement ? bodyElement.innerText : '';

        if (sender) {
          const senderStr = sender.name ? 
            `${sender.name} <${sender.emailAddress}>` : 
            sender.emailAddress;
          participants.add(senderStr);
        }

        if (body && body.length > 20) {
          const senderName = sender?.name || sender?.emailAddress || 'Unknown';
          fullText += `From: ${senderName}\n${body}\n\n`;
        }

        const dateStr = msgView.getDateString?.();
        if (dateStr) latestDate = dateStr;
      } catch (err) {
        console.warn('metldr: error extracting message:', err);
      }
    }

    fullText = fullText.split('\n').filter(line => line.trim().length > 0).join('\n');

    if (!fullText || fullText.length < 50) {
      console.log('metldr: email text too short, skipping');
      return;
    }

    const metadata = {
      subject,
      participants: Array.from(participants),
      emailCount: messages.length,
      date: latestDate,
      timestamp: new Date().toISOString()
    };

    console.log('metldr: extracted metadata:', metadata);

    const threadElement = threadView.getElement?.();

    let loadingDiv = null;
    const loadingTimer = setTimeout(() => {
      loadingDiv = UIService.createLoadingIndicator(threadElement);
      const header = this.findInjectionPoint(threadElement);
      if (header) {
        UIService.injectLoading(header, loadingDiv);
      }
    }, 100);

    console.log('metldr: requesting summary from background...');
    const summary = await this.getSummaryFromBackground(fullText, threadId, metadata);
    console.log('metldr: summary response:', summary);

    clearTimeout(loadingTimer);
    if (loadingDiv) loadingDiv.remove();

    if (summary) {
      console.log('metldr: creating summary card');
      const summaryCard = UIService.createSummaryCard(summary, threadId);
      const header = this.findInjectionPoint(threadElement);
      if (header) {
        UIService.injectSummary(header, summaryCard);
        this.attachRegenerateListener(summaryCard, threadId, fullText, metadata);
      }
    }
  }

  findInjectionPoint(threadElement) {
    if (threadElement) {
      const header = threadElement.querySelector('.gH') || 
                     threadElement.querySelector('.gE') ||
                     threadElement.querySelector('[data-thread-perm-id]');
      if (header) return header;
    }

    return document.querySelector('.gH') || 
           document.querySelector('.gE') ||
           document.querySelector('[data-thread-perm-id]') ||
           document.querySelector('div[role="main"]');
  }

  getThreadIdFromUrl() {
    const hash = window.location.hash;
    const hashMatch = hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/);
    if (hashMatch) return hashMatch[1];

    const url = new URL(window.location.href);
    return url.searchParams.get('msgid') || url.searchParams.get('tid') || null;
  }

  attachRegenerateListener(summaryCard, threadId, emailText, metadata) {
    const regenerateBtn = summaryCard.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        await this.regenerateSummary(threadId, emailText, metadata);
      });
    }
  }

  async getSummaryFromBackground(emailText, emailId, metadata = null, forceRegenerate = false) {
    if (!chrome?.runtime?.sendMessage) return null;

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

  async regenerateSummary(threadId, emailText, metadata) {
    if (this.isRegenerating) return;
    this.isRegenerating = true;

    try {
      const existing = document.querySelector('.metldr-summary');
      if (existing) {
        existing.style.transition = 'opacity 0.2s, transform 0.2s';
        existing.style.opacity = '0';
        existing.style.transform = 'scale(0.95)';
        await new Promise(resolve => setTimeout(resolve, 200));
        existing.remove();
      }

      const header = this.findInjectionPoint(null);
      if (!header) return;

      const loadingDiv = UIService.createLoadingIndicator(document);
      UIService.injectLoading(header, loadingDiv);

      const summary = await this.getSummaryFromBackground(emailText, threadId, metadata, true);
      if (loadingDiv) loadingDiv.remove();

      if (summary) {
        const summaryCard = UIService.createSummaryCard(summary, threadId);
        UIService.injectSummary(header, summaryCard);
        this.attachRegenerateListener(summaryCard, threadId, emailText, metadata);
      }
    } finally {
      this.isRegenerating = false;
    }
  }

  destroy() {
    this.processedThreads.clear();
    replyPanel.hide();
  }
}

export const emailExtractor = new EmailExtractor();
