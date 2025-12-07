import { gsap } from 'gsap';
import { formatTime } from '../lib/textUtils.js';

export const THEME_COLORS = {
  default: {
    primary: 'oklch(0.55 0.12 240)',
    secondary: 'oklch(0.50 0.10 280)',
    accent: 'oklch(0.53 0.11 190)',
    bg: 'oklch(0.14 0.01 265)',
    bgSecondary: 'oklch(0.18 0.01 265)',
    text: 'oklch(0.75 0.01 265)',
    textMuted: 'oklch(0.50 0.01 265)',
    border: 'oklch(0.28 0.01 265 / 0.3)',
    borderSubtle: 'oklch(0.28 0.01 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.15)',
  },
  light: {
    primary: 'oklch(0.38 0.20 240)',
    secondary: 'oklch(0.42 0.16 280)',
    accent: 'oklch(0.45 0.17 190)',
    bg: 'oklch(1.00 0.00 0)',
    bgSecondary: 'oklch(0.975 0.00 265)',
    text: 'oklch(0.18 0.02 265)',
    textMuted: 'oklch(0.58 0.01 265)',
    border: 'oklch(0.80 0.01 265 / 0.3)',
    borderSubtle: 'oklch(0.80 0.01 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.08)',
  },
  cyberpunk: {
    primary: 'oklch(0.75 0.22 200)',
    secondary: 'oklch(0.60 0.24 340)',
    accent: 'oklch(0.84 0.16 100)',
    bg: 'oklch(0.15 0.01 265)',
    bgSecondary: 'oklch(0.19 0.01 265)',
    text: 'oklch(0.88 0.02 265)',
    textMuted: 'oklch(0.50 0.02 265)',
    border: 'oklch(0.75 0.22 200 / 0.2)',
    borderSubtle: 'oklch(0.75 0.22 200 / 0.1)',
    shadow: 'oklch(0.75 0.22 200 / 0.10)',
  },
  catppuccin: {
    primary: 'oklch(0.87 0.04 30)',
    secondary: 'oklch(0.72 0.13 290)',
    accent: 'oklch(0.77 0.12 35)',
    bg: 'oklch(0.19 0.02 265)',
    bgSecondary: 'oklch(0.23 0.02 265)',
    text: 'oklch(0.87 0.03 250)',
    textMuted: 'oklch(0.54 0.03 250)',
    border: 'oklch(0.87 0.04 30 / 0.2)',
    borderSubtle: 'oklch(0.87 0.04 30 / 0.1)',
    shadow: 'oklch(0.87 0.04 30 / 0.15)',
  },
  gruvbox: {
    primary: 'oklch(0.66 0.15 45)',
    secondary: 'oklch(0.68 0.12 150)',
    accent: 'oklch(0.75 0.14 80)',
    bg: 'oklch(0.22 0.01 70)',
    bgSecondary: 'oklch(0.26 0.01 70)',
    text: 'oklch(0.86 0.04 70)',
    textMuted: 'oklch(0.58 0.02 70)',
    border: 'oklch(0.66 0.15 45 / 0.3)',
    borderSubtle: 'oklch(0.66 0.15 45 / 0.15)',
    shadow: 'oklch(0.66 0.15 45 / 0.2)',
  },
};

export class UIService {
  static currentThemeName = 'default';
  static currentTheme = THEME_COLORS.default;
  static listeners = [];

  static ANIMATION_STYLE_ID = 'metldr-animations';
  static POPUP_ANIMATION_STYLE_ID = 'metldr-word-popup-animations';

  static async loadFromStorage() {
    try {
      const result = await chrome.storage.local.get('theme');
      const themeName = result.theme || 'default';
      this.setTheme(themeName);
      return themeName;
    } catch (error) {
      console.error('metldr: failed to load theme from storage:', error);
      this.currentThemeName = 'default';
      this.currentTheme = THEME_COLORS.default;
      return 'default';
    }
  }

  static setTheme(themeName) {
    if (!THEME_COLORS[themeName]) {
      console.warn('metldr: unknown theme:', themeName, 'falling back to default');
      themeName = 'default';
    }
    this.currentThemeName = themeName;
    this.currentTheme = THEME_COLORS[themeName];
    this.notifyListeners();
  }

  static onChange(callback) {
    this.listeners.push(callback);
    return () => {
      this.listeners = this.listeners.filter(cb => cb !== callback);
    };
  }

  static notifyListeners() {
    this.listeners.forEach(cb => cb(this.currentThemeName, this.currentTheme));
  }

  static hexToRgb(hex) {
    const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
    return result
      ? `${parseInt(result[1], 16)}, ${parseInt(result[2], 16)}, ${parseInt(result[3], 16)}`
      : hex;
  }

  static init() {
    if (document.head) {
      this.injectSummaryAnimations();
      this.injectPopupAnimations();
    } else {
      document.addEventListener('DOMContentLoaded', () => {
        this.injectSummaryAnimations();
        this.injectPopupAnimations();
      });
    }
  }

  static injectSummaryAnimations() {
    if (!document.head) return;
    if (document.getElementById(this.ANIMATION_STYLE_ID)) return;

    const style = document.createElement('style');
    style.id = this.ANIMATION_STYLE_ID;
    style.textContent = `
      @keyframes statusPulse {
        0%, 100% {
          opacity: 0.85;
          box-shadow: 0 0 8px currentColor, 0 0 12px currentColor;
        }
        50% {
          opacity: 1;
          box-shadow: 0 0 16px currentColor, 0 0 24px currentColor, 0 0 32px currentColor;
        }
      }
    `;
    document.head.appendChild(style);
  }

  static injectPopupAnimations() {
    if (!document.head) return;
    if (document.getElementById(this.POPUP_ANIMATION_STYLE_ID)) return;

    const style = document.createElement('style');
    style.id = this.POPUP_ANIMATION_STYLE_ID;
    style.textContent = `
      @keyframes metldr-popup-enter {
        from {
          opacity: 0;
          transform: scale(0.85) translateY(-6px);
        }
        to {
          opacity: 1;
          transform: scale(1) translateY(0);
        }
      }
      
      @keyframes metldr-fade-in {
        from {
          opacity: 0;
          transform: translateY(-8px);
        }
        to {
          opacity: 1;
          transform: translateY(0);
        }
      }
      
      @keyframes metldr-spin {
        to {
          transform: rotate(360deg);
        }
      }
      
      .metldr-definitions-scroll::-webkit-scrollbar {
        width: 4px;
      }
      
      .metldr-definitions-scroll::-webkit-scrollbar-track {
        background: transparent;
      }
      
      .metldr-definitions-scroll::-webkit-scrollbar-thumb {
        background: var(--metldr-primary, #00f0ff);
        border-radius: 2px;
        opacity: 0.5;
      }
      
      .metldr-definitions-scroll::-webkit-scrollbar-thumb:hover {
        opacity: 0.8;
      }
      
      .metldr-definitions-scroll {
        scrollbar-width: thin;
        scrollbar-color: var(--metldr-primary) transparent;
      }
    `;
    document.head.appendChild(style);
  }

  static escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  static formatTime(ms) {
    return formatTime(ms);
  }

  static createLoadingIndicator(threadElement) {
    const theme = this.currentTheme;

    const loading = document.createElement('div');
    loading.className = 'metldr-loading';
    loading.style.cssText = `
      position: relative;
      margin: 16px 0;
      padding: 12px 16px;
      background: ${theme.bgSecondary};
      border: 1.5px solid ${theme.border};
      border-radius: 12px;
      box-shadow: 0 4px 12px ${theme.shadow};
      opacity: 0;
      transform: scale(0.985);
    `;

    loading.innerHTML = `
      <div style="
        display: flex;
        align-items: center;
        gap: 12px;
      ">
        <div class="metldr-spinner" style="
          width: 16px;
          height: 16px;
          border: 2.5px solid ${theme.border};
          border-top-color: ${theme.primary};
          border-radius: 50%;
          animation: spin 0.7s linear infinite;
        "></div>
        <span style="
          color: ${theme.text};
          font-size: 13px;
          font-weight: 600;
          letter-spacing: 0.01em;
          -webkit-font-smoothing: antialiased;
        ">generating summary...</span>
      </div>
      <style>
        @keyframes spin {
          to { transform: rotate(360deg); }
        }
      </style>
    `;

    return loading;
  }

  static injectLoading(emailHeader, loading) {
    if (!emailHeader) {
      console.error('metldr: could not find email header for loading indicator');
      return loading;
    }

    let emailContainer = emailHeader.parentNode;
    while (emailContainer && !emailContainer.classList.contains('ii') &&
      !emailContainer.classList.contains('nH') &&
      !emailContainer.classList.contains('adn')) {
      emailContainer = emailContainer.parentNode;
      if (!emailContainer || emailContainer === document.body) {
        emailContainer = emailHeader.parentNode;
        break;
      }
    }

    const bodyElement = emailContainer.querySelector('.a3s.aiL') ||
      emailContainer.querySelector('.ii.gt') ||
      emailContainer.querySelector('[dir="ltr"]') ||
      emailHeader.nextElementSibling;

    let targetParent = null;
    if (bodyElement && bodyElement.parentNode) {
      targetParent = bodyElement.parentNode;
      targetParent.insertBefore(loading, bodyElement);
    } else {
      targetParent = emailHeader.parentNode;
      targetParent.insertBefore(loading, emailHeader.nextSibling);
    }

    if (targetParent) {
      targetParent.style.setProperty('overflow', 'visible', 'important');
    }

    gsap.to(loading, {
      opacity: 1,
      scale: 1,
      duration: 0.4,
      ease: 'back.out(1.7)'
    });

    const primaryRgb = this.hexToRgb(this.currentTheme.primary);

    gsap.to(loading, {
      boxShadow: `
        0 0 8px rgba(${primaryRgb}, 0.6),
        0 0 16px rgba(${primaryRgb}, 0.4),
        0 0 24px rgba(${primaryRgb}, 0.2)
      `,
      duration: 1.5,
      repeat: -1,
      yoyo: true,
      ease: 'sine.inOut'
    });

    return loading;
  }

  static createSummaryCard(summary, threadId) {
    const theme = this.currentTheme;
    const summaryText = summary.summary || '';
    const actions = (summary.action_items || [])
      .map(a => (typeof a === 'string' ? a.trim() : ''))
      .filter(a => a && a.toLowerCase() !== 'none');
    const dates = summary.dates || [];
    const confidence = summary.confidence || 'medium';
    const modelName = summary.model || 'unknown';
    const intent = summary.intent || null;

    const confidenceColors = {
      high: '#10b981',
      medium: '#f59e0b',
      low: '#ef4444'
    };
    const confColor = confidenceColors[confidence] || '#6b7280';

    const intentColors = {
      'invoice': { bg: '#10b981', text: '#ffffff' },
      'receipt': { bg: '#059669', text: '#ffffff' },
      'payment': { bg: '#047857', text: '#ffffff' },
      'refund': { bg: '#34d399', text: '#064e3b' },
      'subscription': { bg: '#6ee7b7', text: '#064e3b' },

      'meeting request': { bg: '#3b82f6', text: '#ffffff' },
      'calendar invite': { bg: '#2563eb', text: '#ffffff' },
      'reminder': { bg: '#60a5fa', text: '#1e3a8a' },
      'reschedule': { bg: '#93c5fd', text: '#1e3a8a' },
      'cancellation': { bg: '#1d4ed8', text: '#ffffff' },

      'flight booking': { bg: '#06b6d4', text: '#ffffff' },
      'hotel reservation': { bg: '#0891b2', text: '#ffffff' },
      'travel itinerary': { bg: '#22d3ee', text: '#164e63' },
      'ticket': { bg: '#67e8f9', text: '#164e63' },
      'reservation': { bg: '#0e7490', text: '#ffffff' },

      'order confirmation': { bg: '#6366f1', text: '#ffffff' },
      'shipping update': { bg: '#818cf8', text: '#1e1b4b' },
      'delivery notice': { bg: '#4f46e5', text: '#ffffff' },
      'tracking': { bg: '#a5b4fc', text: '#1e1b4b' },
      'return/exchange': { bg: '#7c3aed', text: '#ffffff' },

      'account alert': { bg: '#f43f5e', text: '#ffffff' },
      'security alert': { bg: '#e11d48', text: '#ffffff' },
      'password reset': { bg: '#fb7185', text: '#881337' },
      'verification': { bg: '#fda4af', text: '#881337' },
      'login notification': { bg: '#be123c', text: '#ffffff' },

      'task assignment': { bg: '#475569', text: '#ffffff' },
      'project update': { bg: '#64748b', text: '#ffffff' },
      'status report': { bg: '#94a3b8', text: '#1e293b' },
      'feedback request': { bg: '#334155', text: '#ffffff' },
      'approval request': { bg: '#1e293b', text: '#ffffff' },

      'personal': { bg: '#ec4899', text: '#ffffff' },
      'introduction': { bg: '#f472b6', text: '#831843' },
      'follow-up': { bg: '#db2777', text: '#ffffff' },
      'thank you': { bg: '#fce7f3', text: '#9d174d' },
      'announcement': { bg: '#be185d', text: '#ffffff' },

      'newsletter': { bg: '#8b5cf6', text: '#ffffff' },
      'marketing': { bg: '#f97316', text: '#ffffff' },
      'promotion': { bg: '#fb923c', text: '#7c2d12' },
      'survey': { bg: '#fdba74', text: '#7c2d12' },
      'invitation': { bg: '#ea580c', text: '#ffffff' },

      'support ticket': { bg: '#14b8a6', text: '#ffffff' },
      'bug report': { bg: '#0d9488', text: '#ffffff' },
      'feature request': { bg: '#2dd4bf', text: '#134e4a' },
      'complaint': { bg: '#0f766e', text: '#ffffff' },
      'resolution': { bg: '#5eead4', text: '#134e4a' },

      'contract': { bg: '#d97706', text: '#ffffff' },
      'legal notice': { bg: '#b45309', text: '#ffffff' },
      'policy update': { bg: '#fbbf24', text: '#78350f' },
      'hr notice': { bg: '#f59e0b', text: '#78350f' },
      'compliance': { bg: '#92400e', text: '#ffffff' },

      'bank statement': { bg: '#047857', text: '#ffffff' },
      'tax document': { bg: '#065f46', text: '#ffffff' },
      'financial report': { bg: '#10b981', text: '#ffffff' },
      'investment update': { bg: '#34d399', text: '#064e3b' },

      'social notification': { bg: '#8b5cf6', text: '#ffffff' },
      'connection request': { bg: '#a78bfa', text: '#2e1065' },
      'mention': { bg: '#7c3aed', text: '#ffffff' },
      'comment': { bg: '#c4b5fd', text: '#2e1065' },

      'satire/joke': { bg: '#fbbf24', text: '#78350f' },
      'spam': { bg: '#991b1b', text: '#ffffff' },
      'phishing attempt': { bg: '#7f1d1d', text: '#fef2f2' },
      'auto-reply': { bg: '#9ca3af', text: '#1f2937' },
      'out of office': { bg: '#6b7280', text: '#ffffff' },
      'forwarded': { bg: '#a1a1aa', text: '#27272a' },
      'thread reply': { bg: '#d4d4d8', text: '#3f3f46' },
      'digest': { bg: '#71717a', text: '#ffffff' },
      'notification': { bg: '#6366f1', text: '#ffffff' },
      'confirmation': { bg: '#06b6d4', text: '#ffffff' },
      'other': { bg: '#52525b', text: '#ffffff' }
    };

    const summaryDiv = document.createElement('div');
    summaryDiv.className = 'metldr-summary';
    summaryDiv.setAttribute('data-metldr-thread', threadId);
    summaryDiv.setAttribute('data-metldr-injected', 'true');
    summaryDiv.setAttribute('data-metldr-persistent', 'true');
    summaryDiv.style.cssText = `
      opacity: 0 !important;
      transform: scale(0.985) !important;
      will-change: opacity, transform !important;
      transition: opacity 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94), 
                  transform 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94) !important;
      position: relative !important;
      z-index: 1 !important;
    `;

    summaryDiv.innerHTML = this._buildSummaryHTML(theme, summaryText, actions, dates,
      confidence, confColor, modelName, summary, threadId, intent, intentColors);

    return summaryDiv;
  }

  static _buildSummaryHTML(theme, summaryText, actions, dates, confidence, confColor, modelName, summary, threadId, intent, intentColors) {
    const intentKey = intent?.toLowerCase();
    const intentStyle = intentColors?.[intentKey] || { bg: theme.bgSecondary, text: theme.textMuted };
    const intentBadge = intent ? `
      <!-- separator -->
      <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
      
      <span title="email type: ${this.escapeHtml(intent)}" style="
        font-size: 10px;
        color: ${intentStyle.text};
        background: ${intentStyle.bg};
        padding: 2px 6px;
        border-radius: 4px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.02em;
        cursor: help;
      ">${this.escapeHtml(intent)}</span>
    ` : '';
    return `
      <div style="
        position: relative;
        width: 100%;
        box-sizing: border-box;
        margin: 24px 0 16px 0;
      ">
        <!-- unified status badge -->
        <div style="
          position: absolute;
          top: -12px;
          left: 16px;
          display: flex;
          align-items: center;
          gap: 8px;
          background: ${theme.bgSecondary};
          padding: 6px 14px;
          border: 0.5px solid ${theme.borderSubtle};
          border-radius: 12px;
          box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
          z-index: 1;
          -webkit-font-smoothing: antialiased;
        ">
          <!-- branding section -->
          <div style="display: flex; align-items: center; gap: 5px;">
            <div title="confidence: ${confidence} • how certain the ai is about this summary's accuracy" style="
              width: 6px;
              height: 6px;
              background: ${confColor};
              border-radius: 50%;
              box-shadow: 0 0 8px ${confColor};
              cursor: help;
            "></div>
            <strong style="
              font-size: 12px;
              font-weight: 600;
              color: ${theme.primary};
              letter-spacing: 0.01em;
            ">metldr</strong>
          </div>
          
          <!-- separator -->
          <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
          
          <!-- model -->
          <span title="model used for this summary" style="
            font-size: 11px;
            color: ${theme.textMuted};
            font-weight: 500;
            font-family: 'JetBrains Mono', 'SF Mono', Consolas, monospace;
            cursor: help;
          ">${this.escapeHtml(modelName)}</span>
          
          <!-- time -->
          ${summary.time_ms ? `
            <!-- separator -->
            <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
            
            <span title="time taken: ${this.formatTime(summary.time_ms)}${summary.cached ? ' • retrieved from cache' : ' • generated fresh'}" style="
              font-size: 11px;
              color: ${theme.textMuted};
              font-weight: 500;
              cursor: help;
            ">${this.formatTime(summary.time_ms)}</span>
          ` : ''}
          
          <!-- intent badge -->
          ${intentBadge}
        </div>
        
        <!-- regenerate button -->
        <button class="metldr-regenerate-btn" data-thread-id="${threadId}" title="regenerate summary with fresh ai analysis" style="
          position: absolute;
          top: -14px;
          right: 16px;
          width: 28px;
          height: 28px;
          display: flex;
          align-items: center;
          justify-content: center;
          background: ${theme.bgSecondary};
          border: 0.5px solid ${theme.borderSubtle};
          border-radius: 50%;
          color: ${theme.primary};
          font-size: 15px;
          font-weight: 600;
          cursor: pointer;
          transition: all 0.2s cubic-bezier(0.22, 1, 0.36, 1);
          box-shadow: 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
          z-index: 1;
          padding: 0;
          -webkit-font-smoothing: antialiased;
        ">↻</button>
        
        <!-- main card with liquid glass -->
        <div style="
          width: 100%;
          box-sizing: border-box;
          background: ${theme.bg};
          border: 0.5px solid ${theme.border};
          border-radius: 16px;
          padding: 16px;
          padding-top: 20px;
          font-family: 'Space Grotesk', -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
          box-shadow: 0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle};
          position: relative;
          -webkit-font-smoothing: antialiased;
          -moz-osx-font-smoothing: grayscale;
          transition: all 0.3s cubic-bezier(0.22, 1, 0.36, 1);
        ">

        ${summaryText ? `
          <div class="metldr-summary-item" style="
            background: ${theme.bgSecondary};
            border-radius: 8px;
            padding: 10px;
            margin-bottom: ${actions.length > 0 || dates.length > 0 ? '6px' : '0'};
            font-size: 13px;
            line-height: 1.5;
            color: ${theme.text};
            font-weight: 400;
            -webkit-font-smoothing: antialiased;
            -moz-osx-font-smoothing: grayscale;
          ">${this.escapeHtml(summaryText)}</div>
        ` : ''}
          
        ${actions.length > 0 ? `
          <div class="metldr-summary-item" style="
            margin-bottom: ${dates.length > 0 ? '6px' : '0'};
            padding: 10px;
            background: ${theme.bgSecondary};
            border-radius: 8px;
          ">
            <div style="
              font-size: 10px;
              color: ${theme.secondary};
              text-transform: uppercase;
              letter-spacing: 0.06em;
              font-weight: 600;
              margin-bottom: 8px;
            ">action items</div>
            <ul style="
              margin: 0;
              padding-left: 0;
              list-style: none;
              color: ${theme.text};
              line-height: 1.5;
              font-size: 13px;
            ">
              ${actions.map(action => `
                <li style="
                  padding-left: 14px;
                  position: relative;
                  margin-bottom: 5px;
                  font-weight: 400;
                ">
                  <span style="
                    position: absolute;
                    left: 0;
                    top: 4px;
                    color: ${theme.secondary};
                    font-size: 7px;
                  ">▸</span>
                  ${this.escapeHtml(action)}
                </li>
              `).join('')}
            </ul>
          </div>
        ` : ''}
          
        ${dates.length > 0 ? `
          <div class="metldr-summary-item" style="
            font-size: 11px;
            color: ${theme.accent};
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 5px;
            font-weight: 500;
            -webkit-font-smoothing: antialiased;
          ">
            ${dates.map(d => `<span style="background: ${theme.bgSecondary}; padding: 3px 8px; border-radius: 5px; font-size: 11px; -webkit-font-smoothing: antialiased;">${this.escapeHtml(d)}</span>`).join('')}
          </div>
        ` : ''}
        </div>
      </div>
      
      <style>
        .metldr-regenerate-btn:hover {
          background: ${theme.bgSecondary} !important;
          border-color: ${theme.primary} !important;
          color: ${theme.primary} !important;
          transform: scale(1.15) rotate(90deg);
          box-shadow: 0 3px 8px ${theme.shadow};
        }
        .metldr-regenerate-btn:active {
          background: ${theme.bg} !important;
          transform: scale(0.9) rotate(90deg);
          box-shadow: 0 1px 2px ${theme.shadow};
          transition: all 0.1s cubic-bezier(0.4, 0, 1, 1);
        }
      </style>
    `;
  }

  static injectSummary(emailHeader, summaryDiv) {
    if (!emailHeader) {
      console.error('metldr: could not find email header for summary');
      return;
    }

    let emailContainer = emailHeader.parentNode;

    while (emailContainer && !emailContainer.classList.contains('ii') &&
      !emailContainer.classList.contains('nH') &&
      emailContainer.tagName !== 'DIV' ||
      (emailContainer.tagName === 'DIV' && emailContainer.children.length < 2)) {
      emailContainer = emailContainer.parentNode;
      if (!emailContainer || emailContainer === document.body) {
        emailContainer = emailHeader.parentNode;
        break;
      }
    }

    const bodyElement = emailContainer.querySelector('.a3s.aiL') ||
      emailContainer.querySelector('.ii.gt') ||
      emailContainer.querySelector('[dir="ltr"]') ||
      emailHeader.nextElementSibling;

    let targetParent = null;
    if (bodyElement && bodyElement.parentNode) {
      targetParent = bodyElement.parentNode;
      targetParent.insertBefore(summaryDiv, bodyElement);
    } else {
      targetParent = emailHeader.parentNode;
      targetParent.insertBefore(summaryDiv, emailHeader.nextSibling);
    }

    if (targetParent) {
      targetParent.style.setProperty('overflow', 'visible', 'important');
    }

    summaryDiv.style.setProperty('opacity', '0', 'important');
    summaryDiv.style.setProperty('transform', 'scale(0.985)', 'important');

    summaryDiv.offsetHeight;

    summaryDiv.style.setProperty('opacity', '1', 'important');
    summaryDiv.style.setProperty('transform', 'scale(1)', 'important');

    setTimeout(() => {
      summaryDiv.style.setProperty('will-change', 'auto', 'important');
    }, 500);
  }

  static updatePopupTheme(container) {
    if (!container) return;

    const popup = container.querySelector('.metldr-popup-body');
    if (!popup) return;

    const theme = this.currentTheme;
    const popupBg = theme.bgSecondary;

    popup.style.setProperty('--metldr-primary', theme.primary, 'important');
    popup.style.setProperty('--metldr-secondary', theme.secondary, 'important');
    popup.style.setProperty('--metldr-text', theme.text, 'important');
    popup.style.setProperty('--metldr-bg-secondary', popupBg, 'important');
    popup.style.setProperty('--metldr-border', theme.border, 'important');

    popup.style.setProperty('background', popupBg, 'important');
    popup.style.setProperty('background-color', popupBg, 'important');
    popup.style.setProperty('border-color', theme.border, 'important');
    popup.style.setProperty('box-shadow', `0 8px 24px ${theme.shadow}, 0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');

    this._updatePopupHeader(popup, theme, popupBg);
    this._updatePopupContent(popup, theme, popupBg);
    this._updateDefinitions(popup, theme, popupBg);
    this._updateAiBadges(popup, theme);

    console.log('[popup] theme updated in real-time:', this.currentThemeName, 'using bg:', popupBg);
  }

  static _updatePopupHeader(popup, theme, popupBg) {
    const header = popup.querySelector('.metldr-popup-header');
    if (header) {
      header.style.setProperty('background', popupBg, 'important');
      header.style.setProperty('background-color', popupBg, 'important');
    }

    const wordSpan = header?.querySelector('span');
    if (wordSpan && wordSpan.textContent && wordSpan.textContent.length < 50 && !wordSpan.textContent.includes('ai')) {
      wordSpan.style.setProperty('color', theme.primary, 'important');
    }
  }

  static _updatePopupContent(popup, theme, popupBg) {
    const content = popup.querySelector('.metldr-popup-content');
    if (content) {
      content.style.setProperty('background', popupBg, 'important');
      content.style.setProperty('background-color', popupBg, 'important');
      content.style.setProperty('color', theme.text, 'important');
    }
  }

  static _updateDefinitions(popup, theme, popupBg) {
    const defsContainer = popup.querySelector('.metldr-definitions-scroll');
    if (defsContainer) {
      defsContainer.style.setProperty('background', popupBg, 'important');
      defsContainer.style.setProperty('background-color', popupBg, 'important');
    }

    const content = popup.querySelector('.metldr-popup-content');
    if (content) {
      const posElements = content.querySelectorAll('[data-element-type="pos"]');
      posElements.forEach(el => {
        el.style.setProperty('color', theme.secondary, 'important');
        el.style.setProperty('border-bottom-color', theme.border, 'important');
      });

      const defElements = content.querySelectorAll('[data-element-type="definition"]');
      defElements.forEach(el => {
        el.style.setProperty('color', theme.text, 'important');
      });

      const defBlocks = content.querySelectorAll('[data-element-type="def-block"]');
      defBlocks.forEach(block => {
        block.style.setProperty('background', popupBg, 'important');
        block.style.setProperty('background-color', popupBg, 'important');
        block.style.setProperty('border-bottom-color', theme.border, 'important');
      });
    }
  }

  static _updateAiBadges(popup, theme) {
    const header = popup.querySelector('.metldr-popup-header');
    const aiBadge = header?.querySelector('span[title="ai generated definition"]');
    if (aiBadge) {
      aiBadge.style.setProperty('color', theme.accent, 'important');
    }
  }

  static updateSummaryTheme() {
    const theme = this.currentTheme;
    console.log('metldr: updating summary theme:', this.currentThemeName);

    this._updateLoadingElement(theme);
    this._updateSummaryCard(theme);

    console.log('metldr: summary theme update completed');
  }

  static _updateLoadingElement(theme) {
    const loadingElement = document.querySelector('.metldr-loading');
    if (loadingElement) {
      loadingElement.style.setProperty('background', theme.bgSecondary, 'important');
      loadingElement.style.setProperty('background-color', theme.bgSecondary, 'important');
      loadingElement.style.setProperty('border-color', theme.border, 'important');
      loadingElement.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}`, 'important');

      const spinner = loadingElement.querySelector('.metldr-spinner');
      if (spinner) {
        spinner.style.setProperty('border-color', theme.border, 'important');
        spinner.style.setProperty('border-top-color', theme.primary, 'important');
      }

      const loadingText = loadingElement.querySelector('span');
      if (loadingText) {
        loadingText.style.setProperty('color', theme.text, 'important');
      }
    }
  }

  static _updateSummaryCard(theme) {
    const existingSummary = document.querySelector('.metldr-summary');
    if (!existingSummary) return;

    this._updateStatusBadge(existingSummary, theme);
    this._updateRegenerateButton(existingSummary, theme);
    this._updateMainCard(existingSummary, theme);
  }

  static _updateStatusBadge(summaryCard, theme) {
    const statusBadge = summaryCard.querySelector('div[style*="position: absolute"]');
    if (statusBadge) {
      statusBadge.style.setProperty('background', theme.bgSecondary, 'important');
      statusBadge.style.setProperty('background-color', theme.bgSecondary, 'important');
      statusBadge.style.setProperty('border-color', theme.borderSubtle, 'important');
      statusBadge.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');

      const brandingText = statusBadge.querySelector('strong');
      if (brandingText) {
        brandingText.style.setProperty('color', theme.primary, 'important');
      }

      const modelText = statusBadge.querySelector('span[title*="model used"]');
      if (modelText) {
        modelText.style.setProperty('color', theme.textMuted, 'important');
      }

      const timeText = statusBadge.querySelector('span[title*="time taken"]');
      if (timeText) {
        timeText.style.setProperty('color', theme.textMuted, 'important');
      }

      const separators = statusBadge.querySelectorAll('span[style*="opacity: 0.5"]');
      separators.forEach(sep => sep.style.setProperty('color', theme.borderSubtle, 'important'));
    }
  }

  static _updateRegenerateButton(summaryCard, theme) {
    const regenerateBtn = summaryCard.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.style.setProperty('background', theme.bgSecondary, 'important');
      regenerateBtn.style.setProperty('background-color', theme.bgSecondary, 'important');
      regenerateBtn.style.setProperty('border-color', theme.borderSubtle, 'important');
      regenerateBtn.style.setProperty('color', theme.primary, 'important');
      regenerateBtn.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');
    }
  }

  static _updateMainCard(summaryCard, theme) {
    const mainCard = summaryCard.querySelector('div[style*="border-radius: 16px"]');
    if (mainCard) {
      mainCard.style.setProperty('background', theme.bg, 'important');
      mainCard.style.setProperty('background-color', theme.bg, 'important');
      mainCard.style.setProperty('border-color', theme.border, 'important');
      mainCard.style.setProperty('box-shadow', `0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');

      const summaryItems = summaryCard.querySelectorAll('.metldr-summary-item');
      summaryItems.forEach((item) => {
        this._updateSummaryItem(item, theme);
      });
    }
  }

  static _updateSummaryItem(item, theme) {
    item.style.setProperty('background', theme.bgSecondary, 'important');
    item.style.setProperty('background-color', theme.bgSecondary, 'important');
    item.style.setProperty('color', theme.text, 'important');

    const actionHeader = item.querySelector('div[style*="text-transform: uppercase"]');
    if (actionHeader) {
      actionHeader.style.setProperty('color', theme.secondary, 'important');
    }

    const bullets = item.querySelectorAll('span[style*="position: absolute"][style*="left: 0"]');
    bullets.forEach(bullet => {
      bullet.style.setProperty('color', theme.secondary, 'important');
    });

    const listItems = item.querySelectorAll('li');
    listItems.forEach(li => li.style.setProperty('color', theme.text, 'important'));

    const dateTags = item.querySelectorAll('span[style*="background:"]');
    dateTags.forEach(tag => {
      tag.style.setProperty('background', theme.bgSecondary, 'important');
      tag.style.setProperty('background-color', theme.bgSecondary, 'important');
      tag.style.setProperty('color', theme.accent, 'important');
    });
  }

  static createSummariseButton(threadId, onClick) {
    const theme = this.currentTheme;

    const container = document.createElement('div');
    container.className = 'metldr-summarise-container';
    container.setAttribute('data-metldr-thread', threadId);
    container.style.cssText = `
      position: relative;
      margin: 16px 0;
      display: flex;
      justify-content: center;
      opacity: 0;
      transform: translateY(6px);
      transition: opacity 0.3s ease, transform 0.3s ease;
    `;

    const button = document.createElement('button');
    button.className = 'metldr-summarise-btn';
    button.innerHTML = `
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/>
        <path d="M20 3v4"/>
        <path d="M22 5h-4"/>
        <path d="M4 17v2"/>
        <path d="M5 18H3"/>
      </svg>
      <span>metldr - summarise email</span>
    `;

    button.style.setProperty('background', theme.bgSecondary, 'important');
    button.style.setProperty('border', `1.5px solid ${theme.border}`, 'important');
    button.style.setProperty('color', theme.primary, 'important');
    button.style.setProperty('box-shadow', `0 2px 8px ${theme.shadow}`, 'important');

    button.style.cssText += `
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      padding: 10px 18px;
      border-radius: 10px;
      font-family: 'Space Grotesk', -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      font-size: 12px;
      font-weight: 600;
      letter-spacing: 0.01em;
      cursor: pointer;
      transition: all 0.2s cubic-bezier(0.25, 0.46, 0.45, 0.94);
      -webkit-font-smoothing: antialiased;
      -moz-osx-font-smoothing: grayscale;
      will-change: transform, box-shadow;
      backface-visibility: hidden;
      transform: translateZ(0);
    `;

    const updateTheme = (newTheme) => {
      button.style.setProperty('background', newTheme.bgSecondary, 'important');
      button.style.setProperty('border', `1.5px solid ${newTheme.border}`, 'important');
      button.style.setProperty('color', newTheme.primary, 'important');
      button.style.setProperty('box-shadow', `0 2px 8px ${newTheme.shadow}`, 'important');
    };

    const unsubscribe = this.onChange((_, newTheme) => updateTheme(newTheme));

    // store cleanup function on container
    container._themeUnsubscribe = unsubscribe;

    let isHovered = false;

    button.addEventListener('mouseenter', () => {
      isHovered = true;
      const t = this.currentTheme;
      button.style.setProperty('transform', 'translateY(-2px) translateZ(0)', 'important');
      button.style.setProperty('box-shadow', `0 6px 16px ${t.shadow}`, 'important');
      button.style.setProperty('border-color', t.primary, 'important');
      button.style.setProperty('background', t.bg, 'important');
    });

    button.addEventListener('mouseleave', () => {
      isHovered = false;
      const t = this.currentTheme;
      button.style.setProperty('transform', 'translateY(0) translateZ(0)', 'important');
      button.style.setProperty('box-shadow', `0 2px 8px ${t.shadow}`, 'important');
      button.style.setProperty('border-color', t.border, 'important');
      button.style.setProperty('background', t.bgSecondary, 'important');
    });

    button.addEventListener('mousedown', () => {
      const t = this.currentTheme;
      button.style.setProperty('transform', 'translateY(0) scale(0.98) translateZ(0)', 'important');
      button.style.setProperty('box-shadow', `0 1px 4px ${t.shadow}`, 'important');
    });

    button.addEventListener('mouseup', () => {
      if (isHovered) {
        const t = this.currentTheme;
        button.style.setProperty('transform', 'translateY(-2px) translateZ(0)', 'important');
        button.style.setProperty('box-shadow', `0 6px 16px ${t.shadow}`, 'important');
      }
    });

    const styleId = 'metldr-summarise-btn-styles';
    if (!document.getElementById(styleId)) {
      const style = document.createElement('style');
      style.id = styleId;
      style.textContent = `
        .metldr-summarise-container.removing {
          opacity: 0 !important;
          transform: translateY(-6px) scale(0.95) translateZ(0) !important;
          transition: opacity 0.25s ease, transform 0.25s ease !important;
        }
      `;
      document.head.appendChild(style);
    }

    button.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();

      container.classList.add('removing');
      setTimeout(() => {
        if (container._themeUnsubscribe) container._themeUnsubscribe();
        if (onClick) onClick();
      }, 200);
    });

    container.appendChild(button);

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        container.style.opacity = '1';
        container.style.transform = 'translateY(0)';
      });
    });

    return container;
  }

  static injectSummariseButton(emailHeader, buttonContainer) {
    if (!emailHeader) {
      console.error('metldr: could not find email header for summarise button');
      return;
    }

    let emailContainer = emailHeader.parentNode;
    while (emailContainer && !emailContainer.classList.contains('ii') &&
      !emailContainer.classList.contains('nH') &&
      !emailContainer.classList.contains('adn')) {
      emailContainer = emailContainer.parentNode;
      if (!emailContainer || emailContainer === document.body) {
        emailContainer = emailHeader.parentNode;
        break;
      }
    }

    const bodyElement = emailContainer.querySelector('.a3s.aiL') ||
      emailContainer.querySelector('.ii.gt') ||
      emailContainer.querySelector('[dir="ltr"]') ||
      emailHeader.nextElementSibling;

    let targetParent = null;
    if (bodyElement && bodyElement.parentNode) {
      targetParent = bodyElement.parentNode;
      targetParent.insertBefore(buttonContainer, bodyElement);
    } else {
      targetParent = emailHeader.parentNode;
      targetParent.insertBefore(buttonContainer, emailHeader.nextSibling);
    }

    if (targetParent) {
      targetParent.style.setProperty('overflow', 'visible', 'important');
    }

    return buttonContainer;
  }
}

export const uiService = UIService;
export const themeManager = UIService;

