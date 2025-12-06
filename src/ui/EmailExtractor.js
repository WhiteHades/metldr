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
    this.activeRequestToken = null;
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

    const requestToken = `${threadId}-${Date.now()}`;
    this.activeRequestToken = requestToken;

    const subject = threadView.getSubject();
    const messages = threadView.getMessageViewsAll();

    if (!messages || messages.length === 0) {
      console.log('metldr: no messages in thread');
      return;
    }

    let fullText = '';
    const participants = new Set();
    let latestDate = null;
    let fromName = null;
    let fromEmail = null;
    let replyTo = null;
    let mailedBy = null;
    let signedBy = null;
    const toRecipients = new Set();
    const ccRecipients = new Set();
    const bccRecipients = new Set();

    const formatContact = (contact) => {
      if (!contact) return null;
      const name = contact.name || contact.fullName || contact.displayName;
      const email = contact.emailAddress || contact.address || contact.email;
      if (name && email) return `${name} <${email}>`;
      return email || name || null;
    };

    const addContacts = (list, target) => {
      if (!Array.isArray(list)) return;
      list.forEach((c) => {
        const formatted = formatContact(c);
        if (formatted) target.add(formatted);
      });
    };

    for (const msgView of messages) {
      try {
        const sender = msgView.getSender();
        const bodyElement = msgView.getBodyElement();
        const body = bodyElement ? bodyElement.innerText : '';

        if (sender) {
          if (!fromName && sender.name) fromName = sender.name;
          if (!fromEmail && sender.emailAddress) fromEmail = sender.emailAddress;

          const senderStr = sender.name ? 
            `${sender.name} <${sender.emailAddress}>` : 
            sender.emailAddress;
          participants.add(senderStr);
        }

        if (!replyTo && typeof msgView.getReplyTo === 'function') {
          try {
            const rt = msgView.getReplyTo();
            if (Array.isArray(rt)) {
              const formatted = formatContact(rt[0]);
              if (formatted) replyTo = formatted;
            } else {
              const formatted = formatContact(rt);
              if (formatted) replyTo = formatted;
            }
          } catch { /* ignore */ }
        }

        if ((!mailedBy || !signedBy) && typeof msgView.getSMTPHeaders === 'function') {
          try {
            const headers = msgView.getSMTPHeaders();
            if (headers?.['mailed-by'] && !mailedBy) mailedBy = headers['mailed-by'];
            if (headers?.['signed-by'] && !signedBy) signedBy = headers['signed-by'];
          } catch { /* ignore */ }
        }

        const recipients = msgView.getRecipients?.();
        if (recipients) {
          addContacts(recipients.to, toRecipients);
          addContacts(recipients.cc, ccRecipients);
          addContacts(recipients.bcc, bccRecipients);
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
      timestamp: new Date().toISOString(),
      sender: fromName || null,
      senderEmail: fromEmail || null,
      from: (fromName || fromEmail) ? [fromName, fromEmail ? `<${fromEmail}>` : null].filter(Boolean).join(' ') : null,
      replyTo: replyTo || null,
      to: toRecipients.size ? Array.from(toRecipients).join(', ') : null,
      cc: ccRecipients.size ? Array.from(ccRecipients).join(', ') : null,
      bcc: bccRecipients.size ? Array.from(bccRecipients).join(', ') : null,
      toList: Array.from(toRecipients),
      ccList: Array.from(ccRecipients),
      bccList: Array.from(bccRecipients),
      mailedBy: mailedBy || null,
      signedBy: signedBy || null
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

    if (this.activeRequestToken !== requestToken) {
      console.log('metldr: stale summary response skipped for thread', threadId);
      return;
    }

    const currentThreadId = this.getThreadIdFromUrl();
    if (currentThreadId && currentThreadId !== threadId) {
      console.log('metldr: thread changed before injection, skipping summary insert');
      return;
    }

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

  async getSummaryFromBackground(emailText, emailId, metadata = null, forceRegenerate = false, allowRetry = true) {
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

        if (response?.needsOllama && allowRetry) {
          const ready = await this.waitForOllamaReady();
          if (ready) {
            return await this.getSummaryFromBackground(emailText, emailId, metadata, forceRegenerate, false);
          }
        }

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

  async waitForOllamaReady(maxAttempts = 12, delayMs = 1000) {
    for (let attempt = 0; attempt < maxAttempts; attempt++) {
      try {
        const response = await new Promise((resolve, reject) => {
          chrome.runtime.sendMessage({ type: 'CHECK_OLLAMA_HEALTH' }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message));
            } else {
              resolve(resp);
            }
          });
        });

        if (response?.success && response.connected) {
          return true;
        }
      } catch (err) {
        if (attempt === maxAttempts - 1) {
          console.warn('metldr: ollama still unavailable after wait:', err.message);
        }
      }
      await new Promise(r => setTimeout(r, delayMs));
    }
    return false;
  }

  async regenerateSummary(threadId, emailText, metadata) {
    if (this.isRegenerating) return;
    this.isRegenerating = true;

    const requestToken = `${threadId}-regen-${Date.now()}`;
    this.activeRequestToken = requestToken;

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

      if (this.activeRequestToken !== requestToken) {
        console.log('metldr: stale regen response skipped for thread', threadId);
        return;
      }

      const currentThreadId = this.getThreadIdFromUrl();
      if (currentThreadId && currentThreadId !== threadId) {
        console.log('metldr: thread changed during regen, skipping insert');
        return;
      }

      if (summary) {
        const summaryCard = UIService.createSummaryCard(summary, threadId);
        UIService.injectSummary(header, summaryCard);
        this.attachRegenerateListener(summaryCard, threadId, emailText, metadata);

        if (replyPanel?.refreshForThread) {
          replyPanel.refreshForThread(threadId);
        }
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
