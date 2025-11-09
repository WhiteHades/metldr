import { gsap } from 'gsap';

console.log('metldr: content script loaded');
console.log('metldr: current url:', window.location.href);
console.log('metldr: hostname:', window.location.hostname);

const isGmail = window.location.hostname.includes('mail.google.com');
console.log('metldr: is gmail?', isGmail);

const processedEmails = new Set();

const injectionTimestamps = new Map();
const INJECTION_COOLDOWN = 3000;

if (!document.getElementById('metldr-animations')) {
  const style = document.createElement('style');
  style.id = 'metldr-animations';
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

loadCurrentTheme();

if (isGmail) {
  console.log('MeTLDR: Gmail detected');
  
  waitForGmail().then(() => {
    console.log('MeTLDR: Gmail ready, initializing...');
    initInjector();
  });
} else {
  console.log('MeTLDR: Not Gmail, skipping');
}

async function waitForGmail() {
  return new Promise((resolve) => {
    let attempts = 0;
    const maxAttempts = 50;
    
    const check = () => {
      const container = document.querySelector('div[role="main"]');
      
      if (container || attempts >= maxAttempts) {
        setTimeout(resolve, 500);
      } else {
        attempts++;
        setTimeout(check, 200);
      }
    };
    
    check();
  });
}

function initInjector() {
  console.log('MeTLDR: initInjector called');
  let lastProcessedUrl = '';

  const processEmails = () => {
    const currentUrl = window.location.href;
    console.log('metldr: mutation detected, url:', currentUrl);
    
    if (currentUrl === lastProcessedUrl) {
      const existingSummary = document.querySelector('.metldr-summary');
      const existingLoading = document.querySelector('.metldr-loading');
      
      if (existingSummary || existingLoading || isProcessing) {
        return;
      }
      
      const threadId = getCurrentThreadId();
      if (threadId) {
        const lastInjection = injectionTimestamps.get(threadId);
        const now = Date.now();
        if (lastInjection && (now - lastInjection) < INJECTION_COOLDOWN) {
          console.log(`metldr: cooldown active (${Math.round((now - lastInjection) / 1000)}s ago), skipping re-injection`);
          return;
        }
      }
      
      console.log('metldr: same url but summary missing, re-processing...');
      setTimeout(() => processCurrentEmail(), 1500);
      return;
    }
    
    lastProcessedUrl = currentUrl;
    const threadId = getCurrentThreadId();
    console.log('metldr: current thread id:', threadId);
    
    if (threadId) {
      const existingSummary = document.querySelector('.metldr-summary');
      if (!existingSummary) {
        console.log('metldr: new thread or summary missing, processing in 1.5s...');
        setTimeout(() => processCurrentEmail(), 1500);
      } else {
        console.log('metldr: summary already exists, skipping');
      }
    }
  };

  const observer = new MutationObserver((mutations) => {
    const isOurMutation = mutations.some(m => 
      m.target.classList?.contains('metldr-summary') ||
      m.target.classList?.contains('metldr-loading') ||
      m.target.closest?.('.metldr-summary') ||
      m.target.closest?.('.metldr-loading')
    );
    
    if (isOurMutation) {
      return;
    }
    
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 500);
  });

  const mainContainer = document.querySelector('div[role="main"]') || document.body;
  console.log('MeTLDR: Main container found?', !!mainContainer);
  
  if (mainContainer) {
    observer.observe(mainContainer, {
      childList: true,
      subtree: true,
      attributes: false
    });
    console.log('MeTLDR: Observer started');
  }

  console.log('MeTLDR: Will scan for emails in 2s...');
  setTimeout(() => {
    console.log('MeTLDR: Scanning for initial email...');
    lastProcessedUrl = window.location.href;
    processCurrentEmail();
  }, 2000);

  initInjector.observer = observer;
  console.log('MeTLDR: initInjector complete');

  hookUrlChanges(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 200);
  });
}

function getCurrentThreadId() {
  const url = window.location.href;
  console.log('metldr: checking url for thread id:', url);
  
  let match = url.match(/#inbox\/([a-zA-Z0-9_-]+)/);
  if (match) {
    const threadId = match[1];
    
    if (/^p\d+$/.test(threadId)) {
      console.log('metldr: ignoring pagination marker:', threadId);
      return null;
    }
    
    if (threadId.length < 10) {
      console.log('metldr: thread id too short, ignoring:', threadId);
      return null;
    }
    
    console.log('metldr: found thread id:', threadId);
    return threadId;
  }
  
  match = url.match(/\/mail\/u\/\d+\/.*#inbox\/([a-zA-Z0-9_-]+)/);
  if (match) {
    const threadId = match[1];
    
    if (/^p\d+$/.test(threadId) || threadId.length < 10) {
      console.log('metldr: ignoring invalid thread id:', threadId);
      return null;
    }
    
    console.log('metldr: found thread id (alt):', threadId);
    return threadId;
  }
  
  console.log('metldr: no thread id found in url');
  return null;
}

function hookUrlChanges(onChange) {
  try {
    if (hookUrlChanges._installed) return;
    hookUrlChanges._installed = true;

    const fire = () => {
      try { onChange(); } catch (e) { console.error('MeTLDR: URL change handler failed', e); }
    };

    const pushState = history.pushState;
    history.pushState = function() { pushState.apply(this, arguments); fire(); };

    const replaceState = history.replaceState;
    history.replaceState = function() { replaceState.apply(this, arguments); fire(); };

    window.addEventListener('popstate', fire);
    window.addEventListener('hashchange', fire);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) fire(); });

    let last = location.href;
    setInterval(() => {
      if (location.href !== last) { last = location.href; fire(); }
    }, 1000);

    console.log('MeTLDR: URL change hooks installed');
  } catch (e) {
    console.error('MeTLDR: Failed to install URL hooks', e);
  }
}

let isProcessing = false;

async function processCurrentEmail() {
  if (isProcessing) {
    return;
  }
  
  const threadId = getCurrentThreadId();
  if (!threadId) {
    isProcessing = false;
    return;
  }
  
  const existingSummary = document.querySelector('.metldr-summary');
  const existingLoading = document.querySelector('.metldr-loading');
  if (existingSummary || existingLoading) {
    console.log('metldr: summary or loading already displayed');
    isProcessing = false;
    return;
  }
  
  const emailContainer = document.querySelector('.nH.if') || 
                          document.querySelector('[data-thread-id]') ||
                          document.querySelector('.gs') ||
                          document.querySelector('div[role="main"]');
  
  console.log('metldr: email container found?', !!emailContainer);
  
  if (!emailContainer) {
    console.log('metldr: no email container found');
    isProcessing = false;
    return;
  }
  
  console.log('metldr: processing current email thread...');
  isProcessing = true;
  
  const timeoutId = setTimeout(() => {
    console.log('metldr: processing timeout, force-resetting flag');
    isProcessing = false;
  }, 30000);
  
  try {
    await processEmailThread(emailContainer);
  } finally {
    clearTimeout(timeoutId);
    isProcessing = false;
  }
}

async function processEmailThread(threadElement) {
  try {
    const metadata = extractEmailMetadata(threadElement);
    
    const emailText = extractEmailText(threadElement);
    
    if (!emailText || emailText.length < 50) {
      console.log('metldr: email text too short or empty');
        return;
      }

    const threadId = getCurrentThreadId();
    
    let loadingDiv = null;
    const loadingTimer = setTimeout(() => {
      loadingDiv = showLoading(threadElement);
    }, 100);
    
    const summary = await getSummaryFromBackground(emailText, threadId, metadata);
    
    clearTimeout(loadingTimer);
    if (loadingDiv) loadingDiv.remove();

    if (summary) {
      injectSummaryUI(threadElement, summary);
    }

  } catch (error) {
    console.error('metldr: failed to process:', error);
  }
}

function extractEmailMetadata(threadElement) {
  const metadata = {
    date: null,
    sender: null,
    senderEmail: null,
    subject: null,
    to: null,
    timestamp: null
  };

  try {
    // extract date/time - gmail uses span with title attribute for full timestamp
    const dateElement = document.querySelector('span.g3[title]') || 
                       document.querySelector('.gH .gK span[title]') ||
                       document.querySelector('[data-tooltip][role="gridcell"] span[title]');
    
    if (dateElement) {
      metadata.date = dateElement.getAttribute('title') || dateElement.textContent?.trim();
      // try to parse timestamp
      if (metadata.date) {
        const parsedDate = new Date(metadata.date);
        if (!isNaN(parsedDate.getTime())) {
          metadata.timestamp = parsedDate.toISOString();
        }
      }
    }

    // extract sender name and email
    const senderElement = document.querySelector('span.gD[email]') || 
                         document.querySelector('.gE.iv.gt span[email]') ||
                         document.querySelector('[email][data-hovercard-id]');
    
    if (senderElement) {
      metadata.senderEmail = senderElement.getAttribute('email');
      metadata.sender = senderElement.getAttribute('name') || senderElement.textContent?.trim();
    }

    // extract subject from h2 or title
    const subjectElement = document.querySelector('h2.hP') || 
                          document.querySelector('.ha h2') ||
                          document.querySelector('[data-thread-perm-id] h2');
    
    if (subjectElement) {
      metadata.subject = subjectElement.textContent?.trim();
    }

    // extract "to" recipients
    const toElement = document.querySelector('.gE.iv.gt .g2') ||
                     document.querySelector('[data-hovercard-id]');
    
    if (toElement) {
      metadata.to = toElement.textContent?.trim();
    }

    console.log('metldr: extracted email metadata:', metadata);
  } catch (error) {
    console.error('metldr: failed to extract metadata:', error);
  }

  return metadata;
}

function extractEmailText(threadElement) {
  console.log('metldr: extracting email text...');
  
  const selectors = [
    '.ii.gt',
    '.a3s.aiL', //  gmail body
    '.ii', //  email content
    '[dir="ltr"]',
    '.gmail_signature'
  ];

  let fullText = '';
  
  for (const selector of selectors) {
    const elements = document.querySelectorAll(selector);
    console.log(`metldr: found ${elements.length} elements with selector ${selector}`);
    
    if (elements.length > 0) {
      for (const element of elements) {
        const text = element.textContent?.trim();
        if (text && text.length > 20 && !text.match(/^[\s\n]*$/)) {
          fullText += text + '\n\n---\n\n';
        }
      }
      
    }
  }
  
  if (fullText.length > 50) {
    console.log('metldr: extracted full email text, length:', fullText.length);
    return fullText;
  }

  const allVisibleText = document.body.textContent;
  if (allVisibleText && allVisibleText.length > 500) {
    console.log('metldr: using full page text, length:', allVisibleText.length);
    return allVisibleText;
  }

  console.warn('metldr: could not extract email text properly');
  return '';
}

// get summary via background script
async function getSummaryFromBackground(emailText, emailId, metadata = null, forceRegenerate = false) {
  return new Promise((resolve) => {
    if (!chrome || !chrome.runtime || !chrome.runtime.sendMessage) {
      console.error('metldr: chrome runtime not available - extension may need reload');
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
          console.error('metldr: background error:', chrome.runtime.lastError);
          resolve(null);
        } else {
          resolve(response?.summary || null);
        }
      });
    } catch (error) {
      console.error('metldr: failed to send message:', error);
      resolve(null);
    }
  });
}

function hexToRgb(hex) {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  return result 
    ? `${parseInt(result[1], 16)}, ${parseInt(result[2], 16)}, ${parseInt(result[3], 16)}`
    : hex;
}

function showLoading(threadElement) {
  const theme = THEME_COLORS[currentTheme] || THEME_COLORS.default;
  
  const loading = document.createElement('div');
  loading.className = 'metldr-loading';
  loading.style.cssText = `
    position: relative;
    margin: 12px 0;
    padding: 10px 12px;
    background: ${theme.bg};
    border: 2px solid ${theme.border};
    border-radius: 10px;
    opacity: 0;
    transform: scale(0.985);
  `;
  
  loading.innerHTML = `
    <div style="
      display: flex;
      align-items: center;
      gap: 10px;
    ">
      <div class="metldr-spinner" style="
        width: 14px;
        height: 14px;
        border: 2px solid ${theme.border};
        border-top-color: ${theme.primary};
        border-radius: 50%;
        animation: spin 0.7s linear infinite;
      "></div>
      <span style="
        color: ${theme.text};
        font-size: 13px;
        font-weight: 500;
        -webkit-font-smoothing: antialiased;
      ">generating summary...</span>
    </div>
    <style>
      @keyframes spin {
        to { transform: rotate(360deg); }
      }
    </style>
  `;

  const emailHeader = document.querySelector('.gE.iv.gt') || 
                       document.querySelector('.gE.iv') ||
                       document.querySelector('[data-message-id]');
  
  if (emailHeader && emailHeader.parentNode) {
    const emailContent = emailHeader.nextSibling;
    if (emailContent) {
      emailContent.style.transition = 'transform 0.4s cubic-bezier(0.34, 1.56, 0.64, 1)';
    }
    
    emailHeader.parentNode.insertBefore(loading, emailHeader.nextSibling);
    
    gsap.to(loading, {
      opacity: 1,
      scale: 1,
      duration: 0.4,
      ease: 'back.out(1.7)'
    });
    
    const primaryRgb = hexToRgb(theme.primary);
    
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
  } else {
    console.error('metldr: could not find email header for loading indicator');
  }

  return loading;
}

const THEME_COLORS = {
  default: {
    primary: '#60a5fa',
    secondary: '#a78bfa',
    accent: '#34d399',
    bg: 'rgba(15, 23, 42, 0.98)',
    bgSecondary: 'rgba(30, 41, 59, 0.98)',
    text: '#e2e8f0',
    textMuted: '#94a3b8',
    border: 'rgba(148, 163, 184, 0.2)',
    glow: 'rgba(96, 165, 250, 0.15)',
    shadow: 'rgba(0, 0, 0, 0.3)',
  },
  cyberpunk: {
    primary: '#00f0ff',
    secondary: '#ff0080',
    accent: '#fcee09',
    bg: 'rgba(0, 0, 0, 0.95)',
    bgSecondary: 'rgba(10, 10, 10, 0.95)',
    text: '#e4e4e7',
    textMuted: '#71717a',
    border: 'rgba(0, 240, 255, 0.3)',
    glow: 'rgba(0, 240, 255, 0.4)',
    shadow: 'rgba(0, 240, 255, 0.3)',
  },
  catppuccin: {
    primary: '#f5e0dc',
    secondary: '#cba6f7',
    accent: '#fab387',
    bg: 'rgba(30, 30, 46, 0.95)',
    bgSecondary: 'rgba(24, 24, 37, 0.95)',
    text: '#cdd6f4',
    textMuted: '#6c7086',
    border: 'rgba(245, 224, 220, 0.2)',
    glow: 'rgba(245, 224, 220, 0.3)',
    shadow: 'rgba(245, 224, 220, 0.15)',
  },
  gruvbox: {
    primary: '#fe8019',
    secondary: '#8ec07c',
    accent: '#fabd2f',
    bg: 'rgba(40, 40, 40, 0.95)',
    bgSecondary: 'rgba(29, 32, 33, 0.95)',
    text: '#ebdbb2',
    textMuted: '#928374',
    border: 'rgba(254, 128, 25, 0.3)',
    glow: 'rgba(254, 128, 25, 0.4)',
    shadow: 'rgba(254, 128, 25, 0.2)',
  },
};

let currentTheme = 'default';

async function loadCurrentTheme() {
  try {
    const result = await chrome.storage.local.get('theme');
    currentTheme = result.theme || 'default';
    console.log('metldr: loaded theme from storage:', currentTheme);
  } catch (error) {
    console.error('metldr: failed to load theme:', error);
    currentTheme = 'default';
  }
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.theme) {
    const newTheme = changes.theme.newValue;
    console.log('MeTLDR: Theme changed to:', newTheme);
    currentTheme = newTheme;
    
    const existingSummary = document.querySelector('.metldr-summary');
    if (existingSummary) {
      console.log('MeTLDR: Refreshing summary with new theme');
      const threadId = getCurrentThreadId();
      if (threadId) {
        existingSummary.remove();
        processCurrentEmail();
      }
    }
  }
});

function injectSummaryUI(threadElement, summary) {
  const existingSummary = threadElement.querySelector('.metldr-summary') || 
                          document.querySelector('.metldr-summary');
  if (existingSummary) {
    return;
  }
  
  console.log('MeTLDR: Injecting summary UI with theme:', currentTheme);

  const summaryText = summary.summary || '';
  const actions = summary.action_items || [];
  const dates = summary.dates || [];
  const confidence = summary.confidence || 'medium';
  const currentThreadId = getCurrentThreadId();
  const theme = THEME_COLORS[currentTheme] || THEME_COLORS.default;

  const confidenceColors = {
    high: '#10b981',
    medium: '#f59e0b',
    low: '#ef4444'
  };
  const confColor = confidenceColors[confidence] || '#6b7280';

  // create card with persistence attributes
  const summaryDiv = document.createElement('div');
  summaryDiv.className = 'metldr-summary';
  summaryDiv.setAttribute('data-metldr-thread', currentThreadId); // mark with thread ID
  summaryDiv.setAttribute('data-metldr-injected', 'true'); // mark as our element
  summaryDiv.setAttribute('data-metldr-persistent', 'true'); // signal to not remove
  summaryDiv.style.cssText = `
    opacity: 0 !important;
    transform: scale(0.985) !important;
    will-change: opacity, transform !important;
    transition: opacity 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94), 
                transform 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94) !important;
    position: relative !important;
    z-index: 10 !important;
  `;
  
  const modelName = summary.model || 'unknown';

  summaryDiv.innerHTML = `
    <div style="
      position: relative;
      margin: 20px 0 12px 0;
    ">
      <!-- unified status badge on left border edge -->
      <div style="
        position: absolute;
        top: -10px;
        left: 12px;
        display: flex;
        align-items: center;
        gap: 8px;
        background: ${theme.bg};
        padding: 4px 12px;
        border: 1px solid ${theme.border};
        border-radius: 6px;
        box-shadow: 0 1px 3px ${theme.shadow};
        z-index: 1;
        -webkit-font-smoothing: antialiased;
      ">
        <!-- branding section -->
        <div style="display: flex; align-items: center; gap: 4px;">
          <div title="confidence: ${confidence} • how certain the ai is about this summary's accuracy" style="
            width: 5px;
            height: 5px;
            background: ${confColor};
            border-radius: 50%;
            box-shadow: 0 0 6px ${confColor}80;
            cursor: help;
          "></div>
          <strong style="
            font-size: 11px;
            font-weight: 600;
            color: ${theme.primary};
            letter-spacing: -0.01em;
          ">metldr</strong>
        </div>
        
        <!-- separator -->
        <span style="color: ${theme.border}; font-size: 10px;">|</span>
        
        <!-- model -->
        <span title="model used for this summary" style="
          font-size: 10px;
          color: ${theme.textMuted};
          font-weight: 500;
          font-family: 'Courier New', monospace;
          cursor: help;
        ">${escapeHtml(modelName)}</span>
        
        <!-- time -->
        ${summary.time_ms ? `
          <!-- separator -->
          <span style="color: ${theme.border}; font-size: 10px;">|</span>
          
          <span title="time taken: ${formatTime(summary.time_ms)}${summary.cached ? ' • retrieved from cache' : ' • generated fresh'}" style="
            font-size: 10px;
            color: ${theme.textMuted};
            font-weight: 500;
            cursor: help;
          ">${formatTime(summary.time_ms)}</span>
        ` : ''}
      </div>
      
      <!-- regenerate button on right border edge -->
      <button class="metldr-regenerate-btn" data-thread-id="${currentThreadId}" title="regenerate summary with fresh ai analysis" style="
        position: absolute;
        top: -12px;
        right: 12px;
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 50%;
        color: ${theme.primary};
        font-size: 13px;
        cursor: pointer;
        transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
        box-shadow: 0 1px 3px ${theme.shadow};
        z-index: 1;
        padding: 0;
        -webkit-font-smoothing: antialiased;
      ">↻</button>
      
      <!-- main card -->
      <div style="
        background: ${theme.bg};
        border: 1px solid ${theme.border};
        border-radius: 10px;
        padding: 12px;
        padding-top: 16px;
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
        box-shadow: 0 2px 8px ${theme.shadow}, 0 1px 2px rgba(0,0,0,0.04);
        position: relative;
        -webkit-font-smoothing: antialiased;
        -moz-osx-font-smoothing: grayscale;
        transition: box-shadow 0.3s ease;
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
        ">${escapeHtml(summaryText)}</div>
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
                ${escapeHtml(action)}
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
          ${dates.map(d => `<span style="background: ${theme.bgSecondary}; padding: 3px 8px; border-radius: 5px; font-size: 11px; -webkit-font-smoothing: antialiased;">${escapeHtml(d)}</span>`).join('')}
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

  // inject into Gmail right after sender info (same as loading indicator)
  const emailHeader = document.querySelector('.gE.iv.gt') || 
                       document.querySelector('.gE.iv') ||
                       document.querySelector('[data-message-id]');
  
  if (emailHeader && emailHeader.parentNode) {
    const emailContent = emailHeader.nextSibling;
    if (emailContent) {
      emailContent.style.transition = 'transform 0.4s cubic-bezier(0.34, 1.56, 0.64, 1)';
    }
    
    emailHeader.parentNode.insertBefore(summaryDiv, emailHeader.nextSibling);
    
    // record injection timestamp to prevent rapid re-injection
    injectionTimestamps.set(currentThreadId, Date.now());
    
    // use synchronous animation (no rAF delay) with !important to force immediate render
    summaryDiv.style.setProperty('opacity', '0', 'important');
    summaryDiv.style.setProperty('transform', 'scale(0.985)', 'important');
    
    // force reflow
    summaryDiv.offsetHeight;
    
    // trigger animation
    summaryDiv.style.setProperty('opacity', '1', 'important');
    summaryDiv.style.setProperty('transform', 'scale(1)', 'important');
    
    // cleanup will-change after animation
    setTimeout(() => {
      summaryDiv.style.setProperty('will-change', 'auto', 'important');
    }, 500);
    
    const regenerateBtn = summaryDiv.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        const threadId = regenerateBtn.getAttribute('data-thread-id');
        console.log('metldr: regenerate button clicked for:', threadId);
        await regenerateSummary(threadId);
      });
    }
  } else {
    console.error('MeTLDR: Could not find email header for summary');
  }
}

// find insertion point in Gmail DOM
function findInsertionPoint() {
  console.log('MeTLDR: Finding insertion point...');
  
  // try Gmail specific selectors to find where to inject
  const selectors = [
    '.nH.if', // expanded email view
    '.gE.iv.gt', // email header
    'div[data-message-id]', // message container
    '.ii.gt', // email body container
    'div[role="main"] div:first-child', // first child of main area
  ];

  for (const selector of selectors) {
    const element = document.querySelector(selector);
    if (element) {
      console.log('MeTLDR: Found insertion point:', selector);
      return element;
    }
  }

  // fallback: find any container that likely contains the email
  const fallback = document.querySelector('div[role="main"] > div:first-child');
  if (fallback) {
    console.log('MeTLDR: Using fallback insertion point');
    return fallback;
  }
  
  console.error('MeTLDR: No insertion point found');
  return null;
}

// escape HTML
function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// format time for display
function formatTime(ms) {
  if (ms < 1000) {
    return `${ms}ms`;
  } else if (ms < 60000) {
    return `${(ms / 1000).toFixed(1)}s`;
  } else {
    const minutes = Math.floor(ms / 60000);
    const seconds = Math.floor((ms % 60000) / 1000);
    return `${minutes}m ${seconds}s`;
  }
}

// debounce flag to prevent duplicate regenerations
let isRegenerating = false;

// regenerate summary (called by button click)
async function regenerateSummary(threadId) {
  if (isRegenerating) {
    console.log('metldr: regeneration already in progress, skipping');
    return;
  }
  
  isRegenerating = true;
  console.log('metldr: regenerating summary for:', threadId);
  
  try {
    // remove existing summary fast fade-out + scale-down
    const existing = document.querySelector('.metldr-summary');
    if (existing) {
      existing.style.transition = 'opacity 0.2s cubic-bezier(0.4, 0, 1, 1), transform 0.2s cubic-bezier(0.4, 0, 1, 1)';
      existing.style.opacity = '0';
      existing.style.transform = 'scale(0.95)';
      await new Promise(resolve => setTimeout(resolve, 200));
      existing.remove();
    }
    
    // process email again with forceRegenerate flag
    const metadata = extractEmailMetadata(document);
    const emailText = extractEmailText(document);
    if (emailText && emailText.length > 50) {
      const emailContainer = document.querySelector('.nH.if') || 
                            document.querySelector('[data-thread-id]') ||
                            document.querySelector('.gs');
      
      if (emailContainer) {
        const loadingDiv = showLoading(emailContainer);
        const summary = await getSummaryFromBackground(emailText, threadId, metadata, true);
        if (loadingDiv) loadingDiv.remove();
        
        if (summary) {
          injectSummaryUI(emailContainer, summary);
        }
      }
    }
  } finally {
    isRegenerating = false;
  }
}
