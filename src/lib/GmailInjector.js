// inject email summaries into gmails

export class GmailInjector {
  constructor(emailProcessor) {
    this.processor = emailProcessor;
    this.injectedEmails = new Set();
    this.observer = null;
    this.summaryCache = new Map();
  }

  // initialise gmail injection
  init() {
    console.log('MeTLDR: Initializing Gmail injector');
    
    // observe gmails DOM for email threads
    this.startObserving();
    
    // inject into existing emails
    this.scanExistingEmails();
  }

  // start mutation observer for gmail's dynamic dom
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

  // extract email content from gmail dom
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

  // inject summary into gmail
  injectSummary(threadElement, summary) {
    const existingSummary = threadElement.querySelector('.metldr-summary');
    if (existingSummary) {
      return;
    }

    // detect gmail theme
    const isDarkMode = this.isGmailDarkMode();

    // create summary container
    const summaryContainer = document.createElement('div');
    summaryContainer.className = 'metldr-summary';
    
    summaryContainer.style.cssText = `
      opacity: 0;
      transform: translateY(-10px) scale(0.98);
    `;

    // build summary HTML
    const html = this.buildSummaryHTML(summary, isDarkMode);
    summaryContainer.innerHTML = html;

    // find insertion point in Gmail
    const insertionPoint = this.findInsertionPoint(threadElement);
    
    if (insertionPoint && insertionPoint.parentNode) {
      insertionPoint.parentNode.insertBefore(summaryContainer, insertionPoint);
      
      this.animateSummaryEntrance(summaryContainer);
      
      console.log('MeTLDR: Summary injected into Gmail');
    }
  }

  isGmailDarkMode() {
    const html = document.documentElement;
    const body = document.body;
    
    return html.classList.contains('dark') || 
           body.classList.contains('dark') ||
           body.getAttribute('data-darkreader-mode') === 'dynamic' ||
           window.matchMedia('(prefers-color-scheme: dark)').matches;
  }

  async animateSummaryEntrance(container) {
    if (!window.gsap) {
      await this.loadGSAP();
    }

    if (window.gsap) {
      const tl = window.gsap.timeline();
      
      tl.to(container, {
        opacity: 1,
        y: 0,
        scale: 1,
        duration: 0.6,
        ease: 'power3.out',
      })
      .from(container.querySelectorAll('.metldr-summary-item'), {
        y: 10,
        opacity: 0,
        duration: 0.4,
        stagger: 0.08,
        ease: 'power2.out',
      }, '-=0.3');
    } else {
      container.style.opacity = '1';
      container.style.transform = 'translateY(0) scale(1)';
      container.style.transition = 'all 0.6s ease-out';
    }
  }

  // load gsap from cdn
  async loadGSAP() {
    return new Promise((resolve, reject) => {
      if (window.gsap) {
        resolve();
        return;
      }

      const script = document.createElement('script');
      script.src = 'https://cdnjs.cloudflare.com/ajax/libs/gsap/3.12.5/gsap.min.js';
      script.onload = () => resolve();
      script.onerror = () => reject(new Error('failed to load gsap'));
      document.head.appendChild(script);
    });
  }

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

  buildSummaryHTML(summary, isDarkMode = false) {
    const bullets = summary.bullets || [];
    const actions = summary.action_items || [];
    const dates = summary.dates || [];
    const confidence = summary.confidence || 'medium';
    const isGenerating = summary.generating || false;

    const theme = isDarkMode ? {
      bg: 'rgba(10, 10, 10, 0.95)',
      border: 'rgba(52, 211, 153, 0.3)',
      glow: 'rgba(52, 211, 153, 0.4)',
      text: '#e4e4e7',
      textMuted: '#a1a1aa',
      accentBg: 'rgba(52, 211, 153, 0.1)',
      accentText: '#34d399',
      badgeBg: 'rgba(52, 211, 153, 0.15)',
    } : {
      bg: 'rgba(255, 255, 255, 0.98)',
      border: 'rgba(16, 185, 129, 0.4)',
      glow: 'rgba(16, 185, 129, 0.5)',
      text: '#27272a',
      textMuted: '#71717a',
      accentBg: 'rgba(16, 185, 129, 0.08)',
      accentText: '#059669',
      badgeBg: 'rgba(16, 185, 129, 0.12)',
    };

    const confidenceColors = {
      high: '#10b981',
      medium: '#f59e0b',
      low: '#ef4444'
    };
    const confColor = confidenceColors[confidence] || '#6b7280';

    return `
      <div style="
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-left: 3px solid ${theme.accentText};
        border-radius: 12px;
        padding: 16px 20px;
        margin: 16px 0;
        font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
        box-shadow: 0 4px 20px ${theme.glow}, 0 0 40px ${theme.glow};
        backdrop-filter: blur(12px);
        position: relative;
        overflow: hidden;
      ">
        ${isGenerating ? `
          <div class="metldr-generating-pulse" style="
            position: absolute;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background: linear-gradient(90deg, transparent 0%, ${theme.glow} 50%, transparent 100%);
            animation: shimmer 2s infinite;
            pointer-events: none;
          "></div>
        ` : ''}
        
        <div style="display: flex; align-items: center; margin-bottom: 14px; gap: 10px;" class="metldr-summary-item">
          <div style="
            width: 10px;
            height: 10px;
            background: ${confColor};
            border-radius: 50%;
            box-shadow: 0 0 12px ${confColor};
            animation: pulse 2s ease-in-out infinite;
          "></div>
          <strong style="
            font-size: 14px;
            font-weight: 700;
            color: ${theme.accentText};
            letter-spacing: -0.01em;
            font-family: 'JetBrains Mono', 'Courier New', monospace;
          ">metldr</strong>
          <span style="
            margin-left: auto;
            font-size: 11px;
            color: ${theme.textMuted};
            padding: 3px 10px;
            background: ${theme.badgeBg};
            border-radius: 12px;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.05em;
          ">${confidence}</span>
        </div>
        
        ${bullets.length > 0 ? `
          <div style="margin-bottom: 14px;" class="metldr-summary-item">
            <ul style="
              margin: 0;
              padding-left: 0;
              list-style: none;
              color: ${theme.text};
              line-height: 1.7;
              font-size: 13px;
            ">
              ${bullets.map(bullet => `
                <li style="
                  padding-left: 22px;
                  position: relative;
                  margin-bottom: 8px;
                ">
                  <span style="
                    position: absolute;
                    left: 0;
                    top: 8px;
                    width: 6px;
                    height: 6px;
                    background: ${theme.accentText};
                    border-radius: 50%;
                    box-shadow: 0 0 6px ${theme.accentText};
                  "></span>
                  ${this.escapeHtml(bullet)}
                </li>
              `).join('')}
            </ul>
          </div>
        ` : ''}

        ${actions.length > 0 ? `
          <div style="
            margin-bottom: 12px;
            padding: 12px 14px;
            background: ${theme.accentBg};
            border-radius: 10px;
            border: 1px solid ${theme.border};
          " class="metldr-summary-item">
            <strong style="
              font-size: 11px;
              color: ${theme.textMuted};
              text-transform: uppercase;
              letter-spacing: 0.08em;
              font-weight: 700;
              display: block;
              margin-bottom: 10px;
            ">⚡ action items</strong>
            <ul style="
              margin: 0;
              padding-left: 0;
              list-style: none;
              color: ${theme.text};
              line-height: 1.6;
              font-size: 12px;
            ">
              ${actions.map(action => `
                <li style="
                  padding-left: 20px;
                  position: relative;
                  margin-bottom: 6px;
                  font-weight: 500;
                ">
                  <span style="
                    position: absolute;
                    left: 0;
                    top: 4px;
                    color: ${theme.accentText};
                    font-weight: 700;
                  ">→</span>
                  ${this.escapeHtml(action)}
                </li>
              `).join('')}
            </ul>
          </div>
        ` : ''}

        ${dates.length > 0 ? `
          <div style="
            font-size: 12px;
            color: ${theme.textMuted};
            display: flex;
            align-items: center;
            gap: 6px;
            font-weight: 500;
          " class="metldr-summary-item">
            <span style="font-size: 14px;">📅</span>
            <span>${dates.join(', ')}</span>
          </div>
        ` : ''}

        <style>
          @keyframes pulse {
            0%, 100% { opacity: 1; transform: scale(1); }
            50% { opacity: 0.6; transform: scale(1.1); }
          }
          
          @keyframes shimmer {
            0% { transform: translateX(-100%); }
            100% { transform: translateX(100%); }
          }
          
          @import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@600;700&display=swap');
        </style>
      </div>
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

