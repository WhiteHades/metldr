// inject email summaries into Gmail UI
// detects email threads, extracts content, processes with Ollama
// displays summaries with smooth animations

export class GmailInjector {
  constructor(emailProcessor) {
    this.processor = emailProcessor;
    this.injectedEmails = new Set();
    this.observer = null;
    this.summaryCache = new Map();
  }

  // initialize Gmail injection
  init() {
    console.log('MeTLDR: Initializing Gmail injector');
    
    // observe Gmail's DOM for email threads
    this.startObserving();
    
    // inject into existing emails
    this.scanExistingEmails();
  }

  // start mutation observer for Gmail's dynamic DOM
  startObserving() {
    const targetNode = document.body;
    
    if (!targetNode) {
      console.error('MeTLDR: Gmail body not found');
      return;
    }

    this.observer = new MutationObserver((mutations) => {
      let shouldScan = false;

      for (const mutation of mutations) {
        // check for new email threads
        if (mutation.type === 'childList' && mutation.addedNodes.length > 0) {
          for (const node of mutation.addedNodes) {
            if (node.nodeType === 1 && this.isEmailThread(node)) {
              shouldScan = true;
              break;
            }
          }
        }
      }

      if (shouldScan) {
        setTimeout(() => this.scanExistingEmails(), 500);
      }
    });

    this.observer.observe(targetNode, {
      childList: true,
      subtree: true
    });

    console.log('MeTLDR: Mutation observer started');
  }

  // check if element is an email thread
  isEmailThread(element) {
    return element.classList?.contains('zA') || 
           element.querySelector?.('.zA') !== null;
  }

  // scan existing emails in Gmail
  async scanExistingEmails() {
    const threads = document.querySelectorAll('.zA');
    
    for (const thread of threads) {
      const threadId = this.getThreadId(thread);
      
      if (!threadId || this.injectedEmails.has(threadId)) {
        continue;
      }

      this.injectedEmails.add(threadId);

      // check if thread is expanded
      const isExpanded = this.isThreadExpanded(thread);
      
      if (isExpanded) {
        await this.processEmailThread(thread);
      }
    }
  }

  // get unique ID for email thread
  getThreadId(threadElement) {
    const link = threadElement.querySelector('a[href*="/mail"]');
    if (link) {
      const url = new URL(link.href, window.location.origin);
      return url.pathname;
    }
    
    return threadElement.getAttribute('data-thread-id') || 
           threadElement.parentElement?.getAttribute('data-thread-id');
  }

  // check if thread is expanded (detailed view)
  isThreadExpanded(threadElement) {
    return document.querySelector('.nH.if') !== null; // Gmail's expanded view container
  }

  // process a single email thread
  async processEmailThread(threadElement) {
    try {
      // extract email content from Gmail DOM
      const emailContent = this.extractEmailContent(threadElement);
      
      if (!emailContent || emailContent.length < 50) {
        console.log('MeTLDR: Email content too short, skipping');
        return;
      }

      // generate summary
      const summary = await this.processor.summarizeEmail(emailContent, {
        useCache: true
      });

      // inject summary into UI
      this.injectSummary(threadElement, summary);

    } catch (error) {
      console.error('MeTLDR: Failed to process email:', error);
    }
  }

  // extract email content from Gmail DOM
  extractEmailContent(threadElement) {
    // try multiple Gmail DOM structures
    const selectors = [
      '.ii.gt', // email body in expanded view
      '.a3s.aiL', // raw email content
      '[data-message-id] .ii', // specific message container
      '.ar' // alternative structure
    ];

    for (const selector of selectors) {
      const element = threadElement.querySelector?.(selector) || 
                      document.querySelector(selector);
      
      if (element) {
        const text = this.getTextContent(element);
        if (text.length > 50) {
          return text;
        }
      }
    }

    // fallback: get all text from thread
    return this.getTextContent(threadElement);
  }

  // get clean text content
  getTextContent(element) {
    // clone to avoid modifying original
    const clone = element.cloneNode(true);
    
    // remove script and style tags
    clone.querySelectorAll('script, style').forEach(el => el.remove());
    
    // get text and clean it
    let text = clone.textContent || clone.innerText || '';
    
    // remove excessive whitespace
    text = text.replace(/\s+/g, ' ').trim();
    
    return text;
  }

  // inject summary into Gmail UI
  injectSummary(threadElement, summary) {
    // check if already injected
    const existingSummary = threadElement.querySelector('.metldr-summary');
    if (existingSummary) {
      return; // already injected
    }

    // create summary container
    const summaryContainer = document.createElement('div');
    summaryContainer.className = 'metldr-summary';
    summaryContainer.style.cssText = `
      background: linear-gradient(135deg, #2a2a3a 0%, #1a1a2a 100%);
      border-left: 3px solid #5a5;
      padding: 16px;
      margin: 12px 0;
      border-radius: 8px;
      animation: fadeIn 0.3s ease-out;
    `;

    // build summary HTML
    const html = this.buildSummaryHTML(summary);
    summaryContainer.innerHTML = html;

    // find insertion point in Gmail
    const insertionPoint = this.findInsertionPoint(threadElement);
    
    if (insertionPoint) {
      insertionPoint.parentNode.insertBefore(summaryContainer, insertionPoint);
      console.log('MeTLDR: Summary injected into Gmail');
    }
  }

  // find best place to inject summary
  findInsertionPoint(threadElement) {
    // try to insert after thread header, before messages
    const selectors = [
      '.gE.iv.gt', // thread header
      '.h7', // alternative header
      '.gK' // another structure
    ];

    for (const selector of selectors) {
      const element = document.querySelector(selector);
      if (element && element.parentNode) {
        return element;
      }
    }

    // fallback: insert at top of thread
    return threadElement.firstChild;
  }

  // build HTML for summary display
  buildSummaryHTML(summary) {
    const bullets = summary.bullets || [];
    const actions = summary.action_items || [];
    const dates = summary.dates || [];
    const confidence = summary.confidence || 'medium';

    const confidenceColor = {
      high: '#5a5',
      medium: '#aa5',
      low: '#a55'
    }[confidence] || '#888';

    return `
      <div style="color: #fff; font-family: system-ui, sans-serif;">
        <div style="display: flex; align-items: center; margin-bottom: 12px;">
          <span style="width: 8px; height: 8px; background: ${confidenceColor}; border-radius: 50%; margin-right: 8px; display: inline-block;"></span>
          <strong style="font-size: 1em; color: #5a5;">MeTLDR Summary</strong>
          <span style="margin-left: auto; font-size: 0.85em; color: #888;">${confidence}</span>
        </div>
        
        ${bullets.length > 0 ? `
          <div style="margin-bottom: 12px;">
            <ul style="margin: 0; padding-left: 20px; color: #ccc; line-height: 1.6;">
              ${bullets.map(bullet => `<li>${this.escapeHtml(bullet)}</li>`).join('')}
            </ul>
          </div>
        ` : ''}

        ${actions.length > 0 ? `
          <div style="margin-bottom: 12px; padding: 8px; background: rgba(90, 125, 89, 0.1); border-radius: 4px;">
            <strong style="font-size: 0.85em; color: #888;">Action Items:</strong>
            <ul style="margin: 8px 0 0 0; padding-left: 20px; color: #ccc; line-height: 1.4;">
              ${actions.map(action => `<li>${this.escapeHtml(action)}</li>`).join('')}
            </ul>
          </div>
        ` : ''}

        ${dates.length > 0 ? `
          <div style="font-size: 0.85em; color: #aaa;">
            📅 ${dates.join(', ')}
          </div>
        ` : ''}
      </div>
      
      <style>
        @keyframes fadeIn {
          from { opacity: 0; transform: translateY(-10px); }
          to { opacity: 1; transform: translateY(0); }
        }
      </style>
    `;
  }

  // escape HTML to prevent XSS
  escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  // stop observing
  destroy() {
    if (this.observer) {
      this.observer.disconnect();
      this.observer = null;
    }
    
    this.injectedEmails.clear();
    this.summaryCache.clear();
    
    console.log('MeTLDR: Gmail injector destroyed');
  }
}

