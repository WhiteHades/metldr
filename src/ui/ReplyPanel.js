import { gsap } from 'gsap';
import { UIService } from './UIService.js';

export class ReplyPanel {
  constructor() {
    this.panel = null;
    this.triggerButton = null;
    this.composeArea = null;
    this.currentEmailId = null;
    this.suggestions = [];
    this.isVisible = false;
    this.isTriggerVisible = false;
    this.themeUnsubscribe = null;
    this.toolbarObserver = null;
    this.watchInterval = null;
    this.injectedToolbars = new WeakSet(); // track which toolbars already have our button
  }

  // start watching for compose toolbars
  startWatching() {
    if (this.watchInterval) return; // already watching
    
    console.log('[ReplyPanel] starting toolbar watcher');
    
    // check immediately
    this._scanForToolbars();
    
    // then check every 500ms
    this.watchInterval = setInterval(() => {
      this._scanForToolbars();
    }, 500);
    
    // also observe DOM for new toolbars
    this.toolbarObserver = new MutationObserver(() => {
      this._scanForToolbars();
    });
    this.toolbarObserver.observe(document.body, {
      childList: true,
      subtree: true
    });
  }

  stopWatching() {
    if (this.watchInterval) {
      clearInterval(this.watchInterval);
      this.watchInterval = null;
    }
    if (this.toolbarObserver) {
      this.toolbarObserver.disconnect();
      this.toolbarObserver = null;
    }
  }

  async _scanForToolbars() {
    // find all visible compose toolbars
    const toolbars = document.querySelectorAll('.bAK');
    
    for (const toolbar of toolbars) {
      // skip if not visible
      if (!this._isToolbarVisible(toolbar)) continue;
      
      // skip if we already injected our button here
      if (toolbar.querySelector('.metldr-reply-trigger')) continue;
      
      // find the editable associated with this toolbar
      const composeContainer = this._findComposeContainer(toolbar);
      if (!composeContainer) continue;
      
      const editable = composeContainer.querySelector('[g_editable="true"]') ||
                       composeContainer.querySelector('.Am.Al.editable[contenteditable="true"]');
      if (!editable || !this._isVisible(editable)) continue;
      
      // get thread id and fetch suggestions
      const threadId = this._getCurrentThreadId();
      if (!threadId) continue;
      
      console.log('[ReplyPanel] found new toolbar, injecting button for thread:', threadId);
      
      // fetch suggestions and inject button
      const response = await this._fetchSuggestions(threadId);
      if (response?.success && response.suggestions?.length) {
        this.suggestions = response.suggestions;
        this.currentEmailId = threadId;
        this.composeArea = composeContainer;
        this._injectButtonIntoToolbar(toolbar);
      }
    }
  }

  _findComposeContainer(toolbar) {
    // walk up from toolbar to find compose container
    let container = toolbar.parentElement;
    let attempts = 0;
    while (container && attempts < 10) {
      // check if this container has an editable
      if (container.querySelector('[g_editable="true"]') ||
          container.querySelector('.Am.Al.editable[contenteditable="true"]')) {
        return container;
      }
      container = container.parentElement;
      attempts++;
    }
    return null;
  }

  _getCurrentThreadId() {
    // try to get thread ID from URL or DOM
    const hash = window.location.hash;
    const match = hash.match(/#(?:inbox|sent|all)\/([a-zA-Z0-9]+)/);
    if (match) return match[1];
    
    // fallback: look for thread ID in DOM
    const threadEl = document.querySelector('[data-thread-perm-id]');
    if (threadEl) return threadEl.getAttribute('data-thread-perm-id');
    
    // last fallback: use a timestamp-based ID
    return 'compose-' + Date.now();
  }

  async show(composeArea, emailId) {
    // this method is now mainly called by EmailExtractor as backup
    // the main injection happens via _scanForToolbars
    
    if (!emailId) {
      emailId = this._getCurrentThreadId();
    }
    
    // store context
    this.currentEmailId = emailId;
    this.composeArea = composeArea;

    const response = await this._fetchSuggestions(emailId);
    if (!response?.success || !response.suggestions?.length) {
      console.log('[ReplyPanel] no suggestions available for', emailId);
      return;
    }

    this.suggestions = response.suggestions;
    
    // find toolbar and inject
    const toolbar = composeArea?.querySelector('.bAK') || this._findVisibleToolbar();
    if (toolbar && !toolbar.querySelector('.metldr-reply-trigger')) {
      this._injectButtonIntoToolbar(toolbar);
    }
  }

  _findVisibleToolbar() {
    const toolbars = document.querySelectorAll('.bAK');
    for (const tb of toolbars) {
      if (this._isToolbarVisible(tb)) return tb;
    }
    return null;
  }

  _showFullPanel() {
    if (!this.composeArea || !this.suggestions?.length) {
      console.log('[ReplyPanel] cannot show panel - missing composeArea or suggestions');
      return;
    }
    
    // if panel already visible, just toggle it off
    if (this.isVisible && this.panel) {
      this._animateOut();
      this.isVisible = false;
      return;
    }
    
    console.log('[ReplyPanel] showing full panel with', this.suggestions.length, 'suggestions');
    
    // keep trigger button visible - don't remove it
    
    this._createPanel(this.composeArea);
    this._positionPanel(this.composeArea);
    this._animateIn();
    this._subscribeToTheme();
    this.isVisible = true;
  }

  // drag functionality removed for cleaner UX

  _injectButtonIntoToolbar(toolbar) {
    if (!toolbar || toolbar.querySelector('.metldr-reply-trigger')) {
      return; // already has our button
    }
    
    console.log('[ReplyPanel] injecting button into toolbar, suggestions:', this.suggestions.length);
    
    // create button
    const btn = document.createElement('div');
    btn.className = 'metldr-reply-trigger wG J-Z-I';
    btn.setAttribute('data-tooltip', 'AI reply suggestions');
    btn.setAttribute('aria-label', 'AI reply suggestions');
    btn.setAttribute('tabindex', '1');
    btn.setAttribute('role', 'button');
    btn.style.cssText = 'user-select: none; margin-left: 12px;';
    
    btn.innerHTML = `
      <div class="J-J5-Ji J-Z-I-Kv-H" style="user-select: none;">
        <div class="J-J5-Ji J-Z-I-J6-H" style="user-select: none; position: relative;">
          <div class="metldr-icon aaA aMZ" style="user-select: none;">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#5f6368" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 3l1.5 5.5L19 10l-5.5 1.5L12 17l-1.5-5.5L5 10l5.5-1.5L12 3z"/>
              <path d="M19 15l.88 3.12L23 19l-3.12.88L19 23l-.88-3.12L15 19l3.12-.88L19 15z" opacity="0.5"/>
            </svg>
          </div>
          <span class="metldr-badge" style="
            position: absolute;
            top: -5px;
            right: -7px;
            min-width: 14px;
            height: 14px;
            padding: 0 3px;
            background: #1a73e8;
            color: #fff;
            font-size: 9px;
            font-weight: 700;
            border-radius: 7px;
            display: flex;
            align-items: center;
            justify-content: center;
            font-family: -apple-system, BlinkMacSystemFont, sans-serif;
            box-shadow: 0 1px 3px rgba(0,0,0,0.3);
            z-index: 1;
          ">${this.suggestions.length}</span>
        </div>
      </div>
    `;
    
    // insert as first child of toolbar (right after formatting options button which is outside .bAK)
    if (toolbar.firstChild) {
      toolbar.insertBefore(btn, toolbar.firstChild);
    } else {
      toolbar.appendChild(btn);
    }
    
    this.triggerButton = btn;
    this.isTriggerVisible = true;
    
    // hover effect
    const iconSvg = btn.querySelector('svg');
    btn.addEventListener('mouseenter', () => {
      if (iconSvg) iconSvg.style.stroke = '#1a73e8';
    });
    btn.addEventListener('mouseleave', () => {
      if (iconSvg) iconSvg.style.stroke = '#5f6368';
    });
    
    // click handler
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      console.log('[ReplyPanel] trigger button clicked');
      // update composeArea to current visible one
      this.composeArea = this._findComposeContainer(toolbar);
      this._showFullPanel();
    });
    
    // animate in
    gsap.from(btn, {
      scale: 0,
      duration: 0.25,
      ease: 'back.out(1.7)'
    });
  }

  _showTriggerButton(composeArea) {
    // legacy method - now uses _injectButtonIntoToolbar
    const toolbar = composeArea?.querySelector('.bAK') || this._findVisibleToolbar();
    if (toolbar) {
      this._injectButtonIntoToolbar(toolbar);
    } else {
      console.log('[ReplyPanel] no toolbar found, using floating trigger');
      this._showFloatingTrigger(composeArea);
    }
  }

  _showFloatingTrigger(composeArea) {
    // fallback: floating button near compose area when toolbar injection fails
    const theme = UIService.currentTheme;
    const rect = composeArea.getBoundingClientRect();
    
    this.triggerButton = document.createElement('div');
    this.triggerButton.className = 'metldr-reply-trigger';
    this.triggerButton.style.cssText = `
      position: fixed;
      z-index: 10000;
      top: ${rect.top - 36}px;
      left: ${rect.left}px;
      display: flex;
      align-items: center;
      justify-content: center;
      width: 32px;
      height: 32px;
      background: ${theme.bgSecondary};
      border: 1.5px solid ${theme.primary};
      border-radius: 50%;
      cursor: pointer;
      box-shadow: 0 4px 12px ${theme.shadow};
      transition: all 0.15s ease;
    `;
    
    this.triggerButton.innerHTML = `
      <span style="font-size: 14px;">✨</span>
      <span style="
        position: absolute;
        top: -4px;
        right: -4px;
        min-width: 14px;
        height: 14px;
        padding: 0 3px;
        background: ${theme.primary};
        color: ${theme.bg};
        font-size: 9px;
        font-weight: 700;
        border-radius: 7px;
        display: flex;
        align-items: center;
        justify-content: center;
      ">${this.suggestions.length}</span>
    `;
    
    document.body.appendChild(this.triggerButton);
    this.isTriggerVisible = true;
    
    this.triggerButton.addEventListener('mouseenter', () => {
      this.triggerButton.style.background = theme.primary;
      this.triggerButton.style.transform = 'scale(1.1)';
    });
    
    this.triggerButton.addEventListener('mouseleave', () => {
      this.triggerButton.style.background = theme.bgSecondary;
      this.triggerButton.style.transform = 'scale(1)';
    });
    
    const self = this;
    this.triggerButton.addEventListener('click', (e) => {
      e.stopPropagation();
      self._showFullPanel();
    });
    
    gsap.from(this.triggerButton, {
      scale: 0,
      duration: 0.25,
      ease: 'back.out(1.7)'
    });
  }

  _showGeneratingHint() {
    const theme = UIService.currentTheme;
    
    const hint = document.createElement('div');
    hint.className = 'metldr-reply-generating';
    hint.style.cssText = `
      position: fixed;
      bottom: 20px;
      right: 20px;
      z-index: 10000;
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      background: ${theme.bgSecondary};
      border: 1px solid ${theme.border};
      border-radius: 8px;
      box-shadow: 0 4px 16px ${theme.shadow};
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      font-size: 12px;
      color: ${theme.textMuted};
      opacity: 0;
      transform: translateY(10px);
    `;
    
    hint.innerHTML = `
      <div style="
        width: 6px;
        height: 6px;
        background: ${theme.primary};
        border-radius: 50%;
        animation: metldr-pulse 1.5s ease-in-out infinite;
      "></div>
      <span>generating reply suggestions...</span>
      <style>
        @keyframes metldr-pulse {
          0%, 100% { opacity: 0.4; transform: scale(0.9); }
          50% { opacity: 1; transform: scale(1.1); }
        }
      </style>
    `;
    
    document.body.appendChild(hint);
    
    gsap.to(hint, {
      opacity: 1,
      y: 0,
      duration: 0.3,
      ease: 'power2.out'
    });
    
    // auto-hide after 3 seconds
    setTimeout(() => {
      gsap.to(hint, {
        opacity: 0,
        y: 10,
        duration: 0.2,
        ease: 'power2.in',
        onComplete: () => hint.remove()
      });
    }, 3000);
  }

  hide() {
    if (this.panel) {
      this._animateOut();
    }
    // don't remove trigger button - let toolbar watcher manage it
    if (this.themeUnsubscribe) {
      this.themeUnsubscribe();
      this.themeUnsubscribe = null;
    }
    this.isVisible = false;
    this.isTriggerVisible = false;
    this.currentEmailId = null;
    this.composeArea = null;
  }

  // close only the panel, keep trigger button visible
  _hidePanel() {
    if (this.panel) {
      this._animateOut();
    }
    this.isVisible = false;
  }

  async _fetchSuggestions(emailId, retries = 2) {
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
        return resp;
      } catch (err) {
        // "receiving end does not exist" means background SW is asleep
        if (attempt < retries && err.message.includes('Receiving end does not exist')) {
          console.log('[ReplyPanel] background not ready, retrying...', attempt + 1);
          // small delay to let SW wake up
          await new Promise(r => setTimeout(r, 200));
          continue;
        }
        console.error('[ReplyPanel._fetchSuggestions]', err.message);
        return { success: false };
      }
    }
    return { success: false };
  }

  _createPanel(composeArea) {
    const theme = UIService.currentTheme;

    // outer container
    this.panel = document.createElement('div');
    this.panel.className = 'metldr-reply-panel';
    this.panel.style.cssText = `
      position: fixed !important;
      z-index: 2147483647 !important;
      opacity: 0;
      transform: translateY(-4px);
      will-change: opacity, transform;
    `;

    // main card (matches summary card style)
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
      cursor: grab;
      user-select: none;
      -webkit-font-smoothing: antialiased;
    `;

    // floating status badge on border (exactly like summary box)
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

    // close button (styled like regenerate button)
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
    
    // close button click handler
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      e.preventDefault();
      this._hidePanel();
    });

    // content area
    const content = document.createElement('div');
    content.className = 'metldr-reply-content';
    content.style.cssText = `
      padding: 20px 12px 10px 12px;
      max-height: 240px;
      overflow-y: auto;
    `;

    // add styles (scrollbar + close button hover)
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
        box-shadow: 0 3px 8px ${theme.shadow};
      }
      .metldr-reply-close:active {
        background: ${theme.bg} !important;
        transform: scale(0.95);
      }
    `;

    // build suggestions
    content.innerHTML = this._buildSuggestionsHTML(theme);

    card.appendChild(badge);
    card.appendChild(closeBtn);
    card.appendChild(style);
    card.appendChild(content);
    this.panel.appendChild(card);

    this._setupDrag(card);
    this._attachOptionListeners(content, composeArea);

    document.body.appendChild(this.panel);
  }

  _buildSuggestionsHTML(theme) {
    // limit to 4 suggestions max
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
        transition: border-color 0.15s ease, box-shadow 0.15s ease;
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
        ">${UIService.escapeHtml(this._truncatePreview(s.body, 100))}</span>
      </div>
    `).join('');
  }

  _attachOptionListeners(content, composeArea) {
    const options = content.querySelectorAll('.metldr-reply-option');
    const self = this;
    
    // inject style for hover and selected states
    if (!document.querySelector('#metldr-option-hover-style')) {
      const style = document.createElement('style');
      style.id = 'metldr-option-hover-style';
      style.textContent = `
        .metldr-reply-option.metldr-hover,
        .metldr-reply-option.metldr-selected {
          border-color: var(--metldr-primary) !important;
          box-shadow: 0 2px 8px var(--metldr-shadow) !important;
        }
      `;
      document.head.appendChild(style);
    }
    
    options.forEach(option => {
      option.addEventListener('mouseenter', () => {
        const t = UIService.currentTheme;
        document.documentElement.style.setProperty('--metldr-primary', t.primary);
        document.documentElement.style.setProperty('--metldr-shadow', t.shadow);
        option.classList.add('metldr-hover');
      });
      
      option.addEventListener('mouseleave', () => {
        // only remove hover if not selected
        if (!option.classList.contains('metldr-selected')) {
          option.classList.remove('metldr-hover');
        }
      });
      
      option.addEventListener('click', (e) => {
        e.stopPropagation();
        e.preventDefault();
        
        const t = UIService.currentTheme;
        const idx = parseInt(option.getAttribute('data-reply-idx'), 10);
        const suggestion = self.suggestions[idx];
        
        if (suggestion) {
          // clear selected from all options first
          options.forEach(o => o.classList.remove('metldr-selected'));
          
          // add selected class (persists even after focus change)
          option.classList.add('metldr-selected');
          option.classList.add('metldr-hover');
          
          self._insertReply(composeArea, suggestion.body);
        }
      });
    });
  }

  _setupDrag(popup) {
    let isDragging = false;
    let hasMoved = false;
    let startX, startY, startLeft, startTop;
    const dragThreshold = 5; // pixels to move before considering it a drag

    popup.addEventListener('mousedown', (e) => {
      // don't initiate drag from close button
      if (e.target.closest('.metldr-reply-close')) {
        return;
      }
      isDragging = true;
      hasMoved = false;
      startX = e.clientX;
      startY = e.clientY;
      const rect = this.panel.getBoundingClientRect();
      startLeft = rect.left;
      startTop = rect.top;
    });

    document.addEventListener('mousemove', (e) => {
      if (!isDragging) return;
      
      const dx = e.clientX - startX;
      const dy = e.clientY - startY;
      
      // only start moving after threshold
      if (Math.abs(dx) > dragThreshold || Math.abs(dy) > dragThreshold) {
        hasMoved = true;
        popup.style.cursor = 'grabbing';
        this.panel.style.left = (startLeft + dx) + 'px';
        this.panel.style.top = (startTop + dy) + 'px';
      }
    });

    document.addEventListener('mouseup', (e) => {
      if (isDragging) {
        // if we moved, prevent click from propagating to suggestions
        if (hasMoved) {
          e.stopPropagation();
        }
        isDragging = false;
        hasMoved = false;
        popup.style.cursor = 'grab';
      }
    });
  }

  _truncatePreview(text, maxLen = 60) {
    if (!text) return '';
    const cleaned = text.replace(/\n+/g, ' ').trim();
    return cleaned.length > maxLen ? cleaned.substring(0, maxLen - 3) + '...' : cleaned;
  }

  _positionPanel(composeArea) {
    if (!this.panel || !composeArea) return;

    const composeRect = composeArea.getBoundingClientRect();
    const panelWidth = 260;
    const panelHeight = this.panel.offsetHeight || 180;

    // position above compose area, aligned to left edge
    let left = composeRect.left;
    let top = composeRect.top - panelHeight - 12;

    // if not enough space above, show below
    if (top < 10) {
      top = composeRect.bottom + 12;
    }

    // keep within viewport horizontally
    if (left + panelWidth > window.innerWidth - 10) {
      left = window.innerWidth - panelWidth - 10;
    }
    if (left < 10) left = 10;

    this.panel.style.left = `${left}px`;
    this.panel.style.top = `${top}px`;
    
    // setup escape and outside click handlers
    this._escapeHandler = (e) => {
      if (e.key === 'Escape') {
        this._hidePanel();
      }
    };
    document.addEventListener('keydown', this._escapeHandler);
    
    this._outsideClickHandler = (e) => {
      // guard: panel must exist and be visible
      if (!this.panel || !this.isVisible) return;
      
      const target = e.target;
      if (!target) return;
      
      // check if click is inside panel
      const clickedPanel = this.panel.contains(target);
      
      // check if click is on trigger button (don't close if toggling)
      const clickedTrigger = target.closest('.metldr-reply-trigger');
      
      // check if click is on any metldr element
      const clickedMetldr = target.closest('[class*="metldr"]');
      
      if (!clickedPanel && !clickedTrigger) {
        // delay slightly to allow click events to complete first
        requestAnimationFrame(() => {
          if (this.isVisible && this.panel) {
            this._hidePanel();
          }
        });
      }
    };
    
    // use pointerdown for better cross-device support, capture phase
    // delay attachment to avoid immediate trigger from the click that opened the panel
    setTimeout(() => {
      document.addEventListener('pointerdown', this._outsideClickHandler, true);
    }, 150);
  }

  _insertReply(composeArea, text) {
    // always search globally first - stored composeArea may be stale after popout
    // g_editable is the most reliable gmail attribute
    let editableBody = null;
    
    // search globally for visible editable - handles popout window case
    const globalSelectors = [
      '[g_editable="true"]',
      '.Am.Al.editable[contenteditable="true"]',
      '.Am.aiL.Al.editable[contenteditable="true"]',
      '[role="textbox"][contenteditable="true"][aria-label="Message Body"]',
    ];
    
    for (const sel of globalSelectors) {
      const els = document.querySelectorAll(sel);
      for (const el of els) {
        if (this._isVisible(el)) {
          editableBody = el;
          console.log('[ReplyPanel] found editable via global search:', sel);
          break;
        }
      }
      if (editableBody) break;
    }
    
    // fallback to stored composeArea if global search failed
    if (!editableBody && composeArea) {
      console.log('[ReplyPanel] global search failed, trying stored composeArea');
      editableBody = composeArea?.querySelector('[g_editable="true"]') ||
                     composeArea?.querySelector('[contenteditable="true"][role="textbox"]') ||
                     composeArea?.querySelector('.Am.Al.editable') ||
                     composeArea?.querySelector('[contenteditable="true"]');
    }
    
    // legacy fallback block (now simplified)
    if (!editableBody) {
      console.log('[ReplyPanel] all search methods failed');
      const legacyEl = document.querySelector('[g_editable="true"]');
      if (legacyEl && this._isVisible(legacyEl)) {
        editableBody = legacyEl;
      }
    }

    if (editableBody) {
      // clear existing content and insert new text
      editableBody.focus();
      
      // convert text to html with proper line breaks
      const htmlContent = text
        .split('\\n')
        .map(line => `<div>${line || '<br>'}</div>`)
        .join('');

      editableBody.innerHTML = htmlContent;

      // trigger input event so gmail knows content changed
      editableBody.dispatchEvent(new Event('input', { bubbles: true }));
      
      console.log('[ReplyPanel] successfully inserted reply');
    } else {
      console.error('[ReplyPanel] could not find editable body in compose area');
    }
  }

  _isVisible(el) {
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && 
           window.getComputedStyle(el).display !== 'none' &&
           window.getComputedStyle(el).visibility !== 'hidden';
  }

  _isToolbarVisible(toolbar) {
    if (!toolbar) return false;
    const rect = toolbar.getBoundingClientRect();
    // toolbar should be visible and have reasonable dimensions
    return rect.width > 50 && rect.height > 10 && 
           window.getComputedStyle(toolbar).display !== 'none' &&
           window.getComputedStyle(toolbar).visibility !== 'hidden';
  }

  _animateIn() {
    if (!this.panel) return;

    gsap.to(this.panel, {
      opacity: 1,
      y: 0,
      duration: 0.15,
      ease: 'power2.out'
    });
  }

  _animateOut() {
    if (!this.panel) return;

    gsap.to(this.panel, {
      opacity: 0,
      y: -2,
      duration: 0.1,
      ease: 'power2.in',
      onComplete: () => {
        this._cleanup();
      }
    });
  }

  _cleanup() {
    if (this.panel) {
      this.panel.remove();
      this.panel = null;
    }
    if (this._outsideClickHandler) {
      document.removeEventListener('pointerdown', this._outsideClickHandler, true);
      this._outsideClickHandler = null;
    }
    if (this._escapeHandler) {
      document.removeEventListener('keydown', this._escapeHandler);
      this._escapeHandler = null;
    }
  }

  _subscribeToTheme() {
    this.themeUnsubscribe = UIService.onChange(() => {
      if (this.panel) {
        this._updateTheme();
      }
    });
  }

  _updateTheme() {
    if (!this.panel) return;

    const theme = UIService.currentTheme;

    // update card
    const card = this.panel.querySelector('.metldr-reply-card');
    if (card) {
      card.style.background = theme.bg;
      card.style.borderColor = theme.border;
      card.style.boxShadow = `0 6px 24px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`;
    }

    // update badge
    const badge = this.panel.querySelector('.metldr-reply-badge');
    if (badge) {
      badge.style.background = theme.bgSecondary;
      badge.style.borderColor = theme.borderSubtle;
      badge.style.boxShadow = `0 2px 8px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`;
      const dot = badge.querySelector('div');
      if (dot) {
        dot.style.background = theme.primary;
        dot.style.boxShadow = `0 0 6px ${theme.primary}`;
      }
      const spans = badge.querySelectorAll('span');
      if (spans[0]) spans[0].style.color = theme.primary;
      if (spans[2]) spans[2].style.color = theme.textMuted;
    }

    // update close button
    const closeBtn = this.panel.querySelector('.metldr-reply-close');
    if (closeBtn) {
      closeBtn.style.background = theme.bgSecondary;
      closeBtn.style.borderColor = theme.borderSubtle;
      closeBtn.style.color = theme.textMuted;
    }

    // update scrollbar styles
    const style = this.panel.querySelector('style');
    if (style) {
      style.textContent = `
        .metldr-reply-content::-webkit-scrollbar { width: 4px; }
        .metldr-reply-content::-webkit-scrollbar-track { background: transparent; }
        .metldr-reply-content::-webkit-scrollbar-thumb { background: ${theme.border}; border-radius: 2px; }
        .metldr-reply-content::-webkit-scrollbar-thumb:hover { background: ${theme.textMuted}; }
      `;
    }

    // update options
    const options = this.panel.querySelectorAll('.metldr-reply-option');
    options.forEach(option => {
      const isSelected = option.classList.contains('metldr-selected');
      option.style.background = theme.bgSecondary;
      option.style.borderColor = isSelected ? theme.primary : theme.borderSubtle;
      
      // update tone span
      const tone = option.querySelector('span:first-child');
      if (tone) {
        tone.style.background = `${theme.primary}15`;
        tone.style.color = theme.primary;
      }
      
      // update preview span
      const preview = option.querySelector('span:last-child');
      if (preview) {
        preview.style.color = theme.text;
      }
    });
  }
}

export const replyPanel = new ReplyPanel();
