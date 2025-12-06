import { gsap } from 'gsap';
import { UIService } from './UIService.js';

export class ReplyPanel {
  constructor() {
    this.sdk = null;
    this.panel = null;
    this.triggerButton = null;
    this.currentComposeView = null;
    this.currentThreadId = null;
    this.suggestions = [];
    this.isVisible = false;
    this.themeUnsubscribe = null;
    this.isLoading = false;
    this.pollInterval = null;
    this.isPopupMode = false;
    
    this.anchorElement = null;
    this.scrollListener = null;
    this.resizeListener = null;
    this.isClosing = false;
    this.lastCloseTime = 0;
  }

  init(sdk) {
    this.sdk = sdk;

    if (!sdk) {
      console.warn('metldr: no sdk provided, reply panel disabled');
      return;
    }

    sdk.Compose.registerComposeViewHandler((composeView) => {
      this.handleComposeView(composeView);
    });

    console.log('metldr: reply panel initialized with inboxsdk');
  }

  async handleComposeView(composeView) {
    console.log('metldr: compose view detected');

    const isInline = composeView.isInlineReplyForm?.() ?? false;
    const isPopup = !isInline;
    console.log('metldr: compose mode:', isPopup ? 'popup' : 'inline');

    let threadId = this.getThreadIdFromUrl();
    console.log('metldr: url-based thread id:', threadId);

    if (!threadId) {
      const threadView = composeView.getThreadView?.();
      if (threadView) {
        threadId = await this.getThreadId(threadView);
      }
    }

    if (!threadId && composeView.getThreadID) {
      threadId = composeView.getThreadID();
      console.log('metldr: sdk thread id (fallback):', threadId);
    }
    
    if (!threadId) {
      console.log('metldr: new compose, skipping suggestions');
      return;
    }

    console.log('metldr: compose thread id:', threadId);

    this.currentComposeView = composeView;
    this.currentThreadId = threadId;
    this.isPopupMode = isPopup;

    this.addComposeButton(composeView);

    const response = await this.fetchSuggestions(threadId);
    if (response?.success && response.suggestions?.length) {
      this.suggestions = response.suggestions;
      console.log('metldr: suggestions ready:', this.suggestions.length);
    } else {
      console.log('metldr: suggestions not ready, will poll');
      this.startPolling(threadId);
    }

    composeView.on('destroy', () => {
      if (this.currentComposeView === composeView) {
        this.stopPolling();
        this.hide();
        this.currentComposeView = null;
        this.isPopupMode = false;
      }
    });
  }

  startPolling(threadId) {
    this.stopPolling();
    let attempts = 0;
    const maxAttempts = 20;
    
    this.pollInterval = setInterval(async () => {
      attempts++;
      const response = await this.fetchSuggestions(threadId);
      
      if (response?.contextInvalidated) {
        console.log('metldr: stopping poll (extension reloaded)');
        this.stopPolling();
        return;
      }
      
      if (response?.success && response.suggestions?.length) {
        this.suggestions = response.suggestions;
        console.log('metldr: suggestions ready after polling:', this.suggestions.length);
        this.stopPolling();
        
        if (this.isVisible && this.panel && this.isLoading) {
          this.updatePanelWithSuggestions();
        }
      } else if (attempts >= maxAttempts) {
        console.log('metldr: gave up polling for suggestions');
        this.stopPolling();
      }
    }, 1000);
  }

  refreshForThread(threadId) {
    this.stopPolling();
    this.suggestions = [];
    this.isLoading = true;

    if (this.isVisible && this.panel) {
      const content = this.panel.querySelector('.metldr-reply-content');
      if (content) {
        content.innerHTML = this.buildSuggestionsHTML(UIService.currentTheme);
      }
    }

    if (threadId) {
      this.startPolling(threadId);
    }
  }

  stopPolling() {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }

  async getThreadId(threadView) {
    if (threadView.getThreadIDAsync) {
      try {
        return await threadView.getThreadIDAsync();
      } catch {
        // fallback
      }
    }
    return this.getThreadIdFromUrl();
  }

  getThreadIdFromUrl() {
    const hash = window.location.hash;
    const match = hash.match(/#[^/]+\/(?:[^/]+\/)?([A-Za-z0-9_-]{16,})$/);
    return match ? match[1] : null;
  }

  addComposeButton(composeView) {
    composeView.addButton({
      title: 'metldr replies',
      iconUrl: this.getIconDataUrl(),
      orderHint: 100,
      onClick: (event) => {
        console.log('metldr: compose button clicked');
        this.showSuggestionsPanel(composeView);
      }
    });

    console.log('metldr: added compose button');
  }

  getIconDataUrl() {
    return 'data:image/svg+xml,' + encodeURIComponent(`
      <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#5f6368" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12 3l1.5 5.5L19 10l-5.5 1.5L12 17l-1.5-5.5L5 10l5.5-1.5L12 3z"/>
        <path d="M19 15l.88 3.12L23 19l-3.12.88L19 23l-.88-3.12L15 19l3.12-.88L19 15z" opacity="0.5"/>
      </svg>
    `);
  }

  showSuggestionsPanel(composeView) {
    if (this.isClosing) return;
    
    const timeSinceClose = Date.now() - this.lastCloseTime;
    if (timeSinceClose < 200) {
      console.log('metldr: ignoring reopen (too soon after close)');
      return;
    }
    
    if (this.isVisible && this.panel) {
      this.animateOut();
      this.isVisible = false;
      return;
    }

    this.isLoading = !this.suggestions?.length;
    console.log('metldr: creating suggestions panel, isPopup:', this.isPopupMode, 'loading:', this.isLoading);
    
    try {
      this.anchorElement = composeView.getElement?.() || null;
    } catch (e) {
      this.anchorElement = null;
    }
    
    this.createPanel(composeView);
    this.updatePosition();
    this.setupPositionListeners();
    this.animateIn();
    this.subscribeToTheme();
    this.isVisible = true;
    console.log('metldr: panel shown, position:', this.panel?.style.left, this.panel?.style.top);
  }

  updatePanelWithSuggestions() {
    if (!this.panel || !this.suggestions?.length) return;
    
    this.isLoading = false;
    const content = this.panel.querySelector('.metldr-reply-content');
    if (content) {
      content.innerHTML = this.buildSuggestionsHTML(UIService.currentTheme);
      this.attachOptionListeners(content, this.currentComposeView);
    }
  }

  async fetchSuggestions(emailId, retries = 2) {
    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        const resp = await new Promise((resolve, reject) => {
          chrome.runtime.sendMessage({
            type: 'GET_REPLY_SUGGESTIONS',
            emailId
          }, (resp) => {
            if (chrome.runtime.lastError) {
              reject(new Error(chrome.runtime.lastError.message));
            } else {
              resolve(resp);
            }
          });
        });
        
        if (resp === undefined && attempt < retries) {
          console.log('metldr: empty response, retrying...', attempt + 1);
          await new Promise(r => setTimeout(r, 200));
          continue;
        }
        
        return resp || { success: false };
      } catch (err) {
        if (err.message.includes('Extension context invalidated')) {
          console.log('metldr: extension reloaded, stopping suggestions fetch');
          return { success: false };
        }
        
        const isRetryable = err.message.includes('Receiving end does not exist') ||
                            err.message.includes('message port closed');
        if (attempt < retries && isRetryable) {
          console.log('metldr: background not ready, retrying...', attempt + 1);
          await new Promise(r => setTimeout(r, 200));
          continue;
        }
        console.error('metldr: fetch suggestions failed:', err.message);
        return { success: false };
      }
    }
    return { success: false };
  }

  createPanel(composeView) {
    const theme = UIService.currentTheme;

    this.panel = document.createElement('div');
    this.panel.className = 'metldr-reply-panel';
    
    this.panel.style.cssText = `
      position: fixed !important;
      z-index: 999999 !important;
      opacity: 0;
      transform-origin: top left;
      will-change: opacity, transform;
    `;

    const card = document.createElement('div');
    card.className = 'metldr-reply-card';
    card.style.cssText = `
      position: relative;
      background: ${theme.bg};
      border: 0.5px solid ${theme.border};
      border-radius: 14px;
      box-shadow: 0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      min-width: 280px;
      max-width: 320px;
      -webkit-font-smoothing: antialiased;
    `;

    // badge
    const badge = document.createElement('div');
    badge.className = 'metldr-reply-badge';
    badge.style.cssText = `
      position: absolute;
      top: -11px;
      left: 14px;
      display: flex;
      align-items: center;
      gap: 6px;
      background: ${theme.bgSecondary};
      padding: 5px 12px;
      border: 0.5px solid ${theme.borderSubtle};
      border-radius: 10px;
      box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      z-index: 1;
    `;
    badge.innerHTML = `
      <div style="width: 6px; height: 6px; background: ${theme.primary}; border-radius: 50%; box-shadow: 0 0 6px ${theme.primary};"></div>
      <span style="font-size: 11px; font-weight: 700; color: ${theme.primary}; letter-spacing: 0.01em;">metldr</span>
      <span style="font-size: 11px; color: ${theme.textMuted}; opacity: 0.5;">/</span>
      <span style="font-size: 11px; color: ${theme.textMuted};">replies</span>
    `;

    // close button
    const closeBtn = document.createElement('button');
    closeBtn.className = 'metldr-reply-close';
    closeBtn.textContent = '×';
    closeBtn.style.cssText = `
      position: absolute;
      top: -12px;
      right: 14px;
      width: 24px;
      height: 24px;
      display: flex;
      align-items: center;
      justify-content: center;
      background: ${theme.bgSecondary};
      border: 0.5px solid ${theme.borderSubtle};
      border-radius: 50%;
      color: ${theme.primary};
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      z-index: 1;
      transition: all 0.2s cubic-bezier(0.22, 1, 0.36, 1);
      box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
      line-height: 1;
    `;
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      this.hidePanel();
    });

    // content
    const content = document.createElement('div');
    content.className = 'metldr-reply-content';
    content.style.cssText = `
      padding: 20px 12px 10px 12px;
      max-height: 240px;
      overflow-y: auto;
    `;

    // styles
    const style = document.createElement('style');
    style.textContent = `
      .metldr-reply-content::-webkit-scrollbar { width: 4px; }
      .metldr-reply-content::-webkit-scrollbar-track { background: transparent; }
      .metldr-reply-content::-webkit-scrollbar-thumb { background: ${theme.border}; border-radius: 2px; }
      .metldr-reply-content::-webkit-scrollbar-thumb:hover { background: ${theme.textMuted}; }
      .metldr-reply-close:hover {
        background: ${theme.bgSecondary} !important;
        border-color: ${theme.primary} !important;
        color: ${theme.primary} !important;
        transform: scale(1.1);
      }
    `;

    content.innerHTML = this.buildSuggestionsHTML(theme);

    card.appendChild(badge);
    card.appendChild(closeBtn);
    card.appendChild(style);
    card.appendChild(content);
    this.panel.appendChild(card);

    this.attachOptionListeners(content, composeView);
    this.setupEventHandlers();

    document.body.appendChild(this.panel);
  }

  buildSuggestionsHTML(theme) {
    if (this.isLoading || !this.suggestions?.length) {
      return `
        <div style="
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          padding: 24px 12px;
          gap: 12px;
        ">
          <div style="
            width: 24px;
            height: 24px;
            border: 2px solid ${theme.borderSubtle};
            border-top-color: ${theme.primary};
            border-radius: 50%;
            animation: metldr-spin 0.8s linear infinite;
          "></div>
          <span style="
            font-size: 12px;
            color: ${theme.textMuted};
          ">generating replies...</span>
        </div>
        <style>
          @keyframes metldr-spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
          }
        </style>
      `;
    }
    
    const limited = this.suggestions.slice(0, 4);
    return limited.map((s, idx) => `
      <div class="metldr-reply-option" data-reply-idx="${idx}" style="
        position: relative;
        padding: 10px;
        padding-top: 14px;
        margin-bottom: ${idx < limited.length - 1 ? '8px' : '0'};
        background: ${theme.bgSecondary};
        border: 0.5px solid ${theme.borderSubtle};
        border-radius: 8px;
        cursor: pointer;
        transition: border-color 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
      ">
        <span style="
          position: absolute;
          top: -8px;
          left: 10px;
          padding: 2px 8px;
          background: ${theme.bgSecondary};
          border: 0.5px solid ${theme.borderSubtle};
          color: ${theme.primary};
          font-size: 9px;
          font-weight: 600;
          border-radius: 4px;
          text-transform: lowercase;
          letter-spacing: 0.02em;
        ">${UIService.escapeHtml(s.tone || 'reply')}</span>
        <span style="
          display: block;
          color: ${theme.text};
          font-size: 12px;
          line-height: 1.5;
          display: -webkit-box;
          -webkit-line-clamp: 2;
          -webkit-box-orient: vertical;
          overflow: hidden;
        ">${UIService.escapeHtml(this.truncatePreview(s.body, 100))}</span>
      </div>
    `).join('');
  }

  attachOptionListeners(content, composeView) {
    const options = content.querySelectorAll('.metldr-reply-option');

    options.forEach(option => {
      const handleEnter = () => {
        const t = UIService.currentTheme;
        option.style.borderColor = t.primary;
        option.style.boxShadow = `0 2px 8px ${t.shadow}`;
        option.style.transform = 'translateY(-1px)';
      };

      const handleLeave = () => {
        const t = UIService.currentTheme;
        option.style.borderColor = t.borderSubtle;
        option.style.boxShadow = 'none';
        option.style.transform = 'none';
      };

      option.addEventListener('mouseenter', handleEnter);
      option.addEventListener('mouseleave', handleLeave);
      option.addEventListener('pointerleave', handleLeave);

      option.addEventListener('click', (e) => {
        e.stopPropagation();
        const idx = parseInt(option.getAttribute('data-reply-idx'), 10);
        const suggestion = this.suggestions[idx];
        if (suggestion) {
          this.insertReply(composeView, suggestion.body);
        }
      });
    });
  }

  insertReply(composeView, text) {
    if (!composeView) {
      console.error('metldr: no compose view available');
      return;
    }

    try {
      const htmlContent = text
        .split('\n')
        .map(line => `<div>${line || '<br>'}</div>`)
        .join('');

      composeView.setBodyHTML(htmlContent);
      console.log('metldr: inserted reply via sdk');
    } catch (err) {
      console.error('metldr: failed to insert reply:', err);
      try {
        composeView.insertTextIntoBodyAtCursor(text);
      } catch (e) {
        console.error('metldr: fallback insert also failed:', e);
      }
    }
  }

  // position logic mirrored from WordPopup.updatePosition()
  updatePosition() {
    if (!this.panel || this.isClosing) return;

    try {
      const panelRect = this.panel.getBoundingClientRect();
      const panelWidth = panelRect.width || 320;
      const panelHeight = panelRect.height || 240;
      
      const viewportWidth = window.innerWidth;
      const viewportHeight = window.innerHeight;
      const padding = 12;

      let finalX, finalY;

      if (this.anchorElement) {
        const anchorRect = this.anchorElement.getBoundingClientRect();
        
        // find compose body for better alignment
        const bodyEl = this.anchorElement.querySelector('[role="textbox"], [contenteditable="true"], .editable');
        const bodyRect = bodyEl ? bodyEl.getBoundingClientRect() : anchorRect;
        
        // center horizontally on the compose body
        const anchorCenterX = bodyRect.left + (bodyRect.width / 2);
        finalX = anchorCenterX - (panelWidth / 2);
        
        // try above compose first (like word popup does below word)
        const aboveY = anchorRect.top - panelHeight - padding;
        const belowY = anchorRect.bottom + padding;

        if (aboveY >= padding) {
          // fits above
          finalY = aboveY;
        } else if (belowY + panelHeight < viewportHeight - padding) {
          // fits below
          finalY = belowY;
        } else if (this.isPopupMode) {
          // popup mode: try left/right of compose dialog
          const leftX = anchorRect.left - panelWidth - padding;
          const rightX = anchorRect.right + padding;
          
          if (leftX >= padding) {
            finalX = leftX;
            finalY = Math.max(padding, anchorRect.top);
          } else if (rightX + panelWidth < viewportWidth - padding) {
            finalX = rightX;
            finalY = Math.max(padding, anchorRect.top);
          } else {
            // no room on sides, overlay with offset
            finalX = anchorRect.left + 20;
            finalY = anchorRect.top + 20;
          }
        } else {
          // inline: clamp to visible viewport
          finalY = Math.max(padding, Math.min(aboveY, viewportHeight - panelHeight - padding));
        }
      } else {
        // no anchor, center on screen
        finalX = (viewportWidth - panelWidth) / 2;
        finalY = (viewportHeight - panelHeight) / 2;
      }

      // viewport bounds clamping (same as WordPopup)
      if (finalX < padding) {
        finalX = padding;
      } else if (finalX + panelWidth > viewportWidth - padding) {
        finalX = viewportWidth - panelWidth - padding;
      }

      if (finalY < padding) {
        finalY = padding;
      } else if (finalY + panelHeight > viewportHeight - padding) {
        finalY = viewportHeight - panelHeight - padding;
      }

      this.panel.style.position = 'fixed';
      this.panel.style.left = finalX + 'px';
      this.panel.style.top = finalY + 'px';
    } catch (error) {
      console.log('metldr: error updating panel position:', error.message);
    }
  }

  // scroll/resize listeners (same pattern as WordPopup)
  setupPositionListeners() {
    this.scrollListener = () => this.updatePosition();
    this.resizeListener = () => this.updatePosition();

    window.addEventListener('scroll', this.scrollListener, { capture: true, passive: true });
    window.addEventListener('resize', this.resizeListener);
  }

  removePositionListeners() {
    if (this.scrollListener) {
      window.removeEventListener('scroll', this.scrollListener, true);
      this.scrollListener = null;
    }
    if (this.resizeListener) {
      window.removeEventListener('resize', this.resizeListener);
      this.resizeListener = null;
    }
  }

  setupEventHandlers() {
    // escape handler
    this.escapeHandler = (e) => {
      if (e.key === 'Escape') this.hidePanel();
    };
    document.addEventListener('keydown', this.escapeHandler);

    // outside click handler
    this.outsideClickHandler = (e) => {
      if (!this.panel || !this.isVisible) return;
      
      if (!this.panel.contains(e.target)) {
        requestAnimationFrame(() => {
          if (this.isVisible && this.panel) this.hidePanel();
        });
      }
    };
    setTimeout(() => {
      document.addEventListener('pointerdown', this.outsideClickHandler, true);
    }, 150);
  }

  truncatePreview(text, maxLen = 60) {
    if (!text) return '';
    const cleaned = text.replace(/\n+/g, ' ').trim();
    return cleaned.length > maxLen ? cleaned.substring(0, maxLen - 3) + '...' : cleaned;
  }

  hide() {
    if (this.panel) this.animateOut();
    if (this.themeUnsubscribe) {
      this.themeUnsubscribe();
      this.themeUnsubscribe = null;
    }
    this.isVisible = false;
    this.currentThreadId = null;
  }

  hidePanel() {
    if (this.panel) this.animateOut();
    this.isVisible = false;
    this.lastCloseTime = Date.now();
  }

  animateIn() {
    if (!this.panel) return;
    
    gsap.fromTo(this.panel,
      { opacity: 0, scale: 0.95 },
      { opacity: 1, scale: 1, duration: 0.15, ease: 'power2.out' }
    );
  }

  animateOut() {
    if (!this.panel || this.isClosing) return;
    
    // prevent position updates during close (fixes jank when anchor element is destroyed)
    this.isClosing = true;
    this.removePositionListeners();
    this.anchorElement = null;
    
    this.panel.style.pointerEvents = 'none';
    
    gsap.to(this.panel, {
      opacity: 0,
      duration: 0.1,
      ease: 'power2.in',
      onComplete: () => this.cleanup()
    });
  }

  cleanup() {
    if (this.panel) {
      this.panel.remove();
      this.panel = null;
    }
    if (this.outsideClickHandler) {
      document.removeEventListener('pointerdown', this.outsideClickHandler, true);
      this.outsideClickHandler = null;
    }
    if (this.escapeHandler) {
      document.removeEventListener('keydown', this.escapeHandler);
      this.escapeHandler = null;
    }
    
    this.isClosing = false;
  }

  subscribeToTheme() {
    this.themeUnsubscribe = UIService.onChange(() => {
      if (this.panel) this.updateTheme();
    });
  }

  updateTheme() {
    if (!this.panel) return;
    const theme = UIService.currentTheme;

    const card = this.panel.querySelector('.metldr-reply-card');
    if (card) {
      card.style.background = theme.bg;
      card.style.borderColor = theme.border;
      card.style.boxShadow = `0 6px 24px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`;
    }

    const badge = this.panel.querySelector('.metldr-reply-badge');
    if (badge) {
      badge.style.background = theme.bgSecondary;
      badge.style.borderColor = theme.borderSubtle;
    }

    const closeBtn = this.panel.querySelector('.metldr-reply-close');
    if (closeBtn) {
      closeBtn.style.background = theme.bgSecondary;
      closeBtn.style.borderColor = theme.borderSubtle;
      closeBtn.style.color = theme.textMuted;
    }

    const options = this.panel.querySelectorAll('.metldr-reply-option');
    options.forEach(option => {
      option.style.background = theme.bgSecondary;
      option.style.borderColor = theme.borderSubtle;
    });
  }
}

export const replyPanel = new ReplyPanel();
