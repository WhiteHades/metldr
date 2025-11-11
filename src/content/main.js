import { gsap } from 'gsap';
import { ContentDetector } from '../lib/ContentDetector.js';

const isGmail = window.location.hostname.includes('mail.google.com');

const processedEmails = new Set();

const injectionTimestamps = new Map();
const INJECTION_COOLDOWN = 3000;

// dwell-time pre-summarisation state
let dwellTimer = 0;
let dwellInterval = null;
let summarisationQueued = false;
let currentPageUrl = window.location.href;
const DWELL_THRESHOLD = 0; // instant for testing (change to 30 for production)

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

// start dwell-time monitoring for pre-summarisation
if (!isGmail) {
  startDwellMonitoring();
}

if (isGmail) {
  waitForGmail().then(() => {
    initInjector();
  });
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
  let lastProcessedUrl = '';

  const processEmails = () => {
    const currentUrl = window.location.href;
    
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
          return;
        }
      }
      
      setTimeout(() => processCurrentEmail(), 1500);
      return;
    }
    
    lastProcessedUrl = currentUrl;
    const threadId = getCurrentThreadId();
    
    if (threadId) {
      const existingSummary = document.querySelector('.metldr-summary');
      if (!existingSummary) {
        setTimeout(() => processCurrentEmail(), 1500);
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
  
  if (mainContainer) {
    observer.observe(mainContainer, {
      childList: true,
      subtree: true,
      attributes: false
    });
  }

  setTimeout(() => {
    lastProcessedUrl = window.location.href;
    processCurrentEmail();
  }, 2000);

  initInjector.observer = observer;

  hookUrlChanges(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 200);
  });
}

function getCurrentThreadId() {
  const url = window.location.href;
  
  let match = url.match(/#inbox\/([a-zA-Z0-9_-]+)/);
  if (match) {
    const threadId = match[1];
    
    if (/^p\d+$/.test(threadId)) {
      return null;
    }
    
    if (threadId.length < 10) {
      return null;
    }
    
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
  const theme = currentTheme; // Use the already-loaded theme object directly
  
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
    primary: 'oklch(0.75 0.18 230)',
    secondary: 'oklch(0.70 0.16 285)',
    accent: 'oklch(0.76 0.15 165)',
    bg: 'oklch(0.10 0.01 265)',
    bgSecondary: 'oklch(0.14 0.01 265 / 0.98)',
    text: 'oklch(0.90 0.02 265)',
    textMuted: 'oklch(0.60 0.02 265)',
    border: 'oklch(0.30 0.02 265 / 0.3)',
    borderSubtle: 'oklch(0.30 0.02 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.4)',
  },
  light: {
    primary: 'oklch(0.55 0.20 230)',
    secondary: 'oklch(0.50 0.18 285)',
    accent: 'oklch(0.56 0.17 165)',
    bg: 'oklch(0.98 0.01 265)',
    bgSecondary: 'oklch(0.94 0.01 265)',
    text: 'oklch(0.20 0.02 265)',
    textMuted: 'oklch(0.50 0.02 265)',
    border: 'oklch(0.30 0.02 265 / 0.3)',
    borderSubtle: 'oklch(0.30 0.02 265 / 0.15)',
    shadow: 'oklch(0 0 0 / 0.15)',
  },
  cyberpunk: {
    primary: 'oklch(0.80 0.25 200)',
    secondary: 'oklch(0.65 0.28 340)',
    accent: 'oklch(0.88 0.20 100)',
    bg: 'oklch(0.05 0.01 265)',
    bgSecondary: 'oklch(0.09 0.01 265 / 0.98)',
    text: 'oklch(0.92 0.02 265)',
    textMuted: 'oklch(0.55 0.02 265)',
    border: 'oklch(0.80 0.25 200 / 0.35)',
    borderSubtle: 'oklch(0.80 0.25 200 / 0.18)',
    shadow: 'oklch(0.80 0.25 200 / 0.35)',
  },
  catppuccin: {
    primary: 'oklch(0.87 0.04 30)',
    secondary: 'oklch(0.72 0.13 290)',
    accent: 'oklch(0.77 0.12 35)',
    bg: 'oklch(0.19 0.02 265)',
    bgSecondary: 'oklch(0.23 0.02 265 / 0.98)',
    text: 'oklch(0.87 0.03 250)',
    textMuted: 'oklch(0.54 0.03 250)',
    border: 'oklch(0.87 0.04 30 / 0.25)',
    borderSubtle: 'oklch(0.87 0.04 30 / 0.12)',
    shadow: 'oklch(0.87 0.04 30 / 0.18)',
  },
  gruvbox: {
    primary: 'oklch(0.66 0.15 45)',
    secondary: 'oklch(0.68 0.12 150)',
    accent: 'oklch(0.75 0.14 80)',
    bg: 'oklch(0.22 0.01 70)',
    bgSecondary: 'oklch(0.26 0.01 70 / 0.98)',
    text: 'oklch(0.86 0.04 70)',
    textMuted: 'oklch(0.58 0.02 70)',
    border: 'oklch(0.66 0.15 45 / 0.35)',
    borderSubtle: 'oklch(0.66 0.15 45 / 0.18)',
    shadow: 'oklch(0.66 0.15 45 / 0.25)',
  },
};

// store both theme name (string) and theme object separately
let currentThemeName = 'default';
let currentTheme = THEME_COLORS.default;

async function loadCurrentTheme() {
  try {
    const result = await chrome.storage.local.get('theme');
    const themeName = result.theme || 'default';
    currentThemeName = themeName;
    currentTheme = THEME_COLORS[themeName] || THEME_COLORS.default;
    console.log('metldr: loaded theme from storage:', themeName, currentTheme);

    // Update any existing summaries with the loaded theme
    updateSummaryTheme();
  } catch (error) {
    console.error('metldr: failed to load theme:', error);
    currentThemeName = 'defaul';
    currentTheme = THEME_COLORS.default;
    updateSummaryTheme();
  }
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.theme) {

    const themeName = changes.theme.newValue;
    currentThemeName = themeName;
    currentTheme = THEME_COLORS[themeName] || THEME_COLORS.default;
    console.log('metldr: theme changed to:', themeName, currentTheme);
    
    updatePopupTheme();
    updateSummaryTheme();
    }
});

function injectSummaryUI(threadElement, summary) {
  const existingSummary = threadElement.querySelector('.metldr-summary') || 
                          document.querySelector('.metldr-summary');
  if (existingSummary) {
    return;
  }
  
  console.log('MeTLDR: Injecting summary UI with theme:', currentThemeName, currentTheme);

  const summaryText = summary.summary || '';
  const actions = summary.action_items || [];
  const dates = summary.dates || [];
  const confidence = summary.confidence || 'medium';
  const currentThreadId = getCurrentThreadId();
  const theme = currentTheme; // Use the already-loaded theme object directly

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
        backdrop-filter: blur(24px) saturate(180%);
        -webkit-backdrop-filter: blur(24px) saturate(180%);
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
          font-family: 'SF Mono', 'Courier New', monospace;
          cursor: help;
        ">${escapeHtml(modelName)}</span>
        
        <!-- time -->
        ${summary.time_ms ? `
          <!-- separator -->
          <span style="color: ${theme.borderSubtle}; font-size: 11px; opacity: 0.5;">•</span>
          
          <span title="time taken: ${formatTime(summary.time_ms)}${summary.cached ? ' • retrieved from cache' : ' • generated fresh'}" style="
            font-size: 11px;
            color: ${theme.textMuted};
            font-weight: 500;
            cursor: help;
          ">${formatTime(summary.time_ms)}</span>
        ` : ''}
      </div>
      
      <!-- regenerate button -->
      <button class="metldr-regenerate-btn" data-thread-id="${currentThreadId}" title="regenerate summary with fresh ai analysis" style="
        position: absolute;
        top: -14px;
        right: 16px;
        width: 28px;
        height: 28px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: ${theme.bgSecondary};
        backdrop-filter: blur(24px) saturate(180%);
        -webkit-backdrop-filter: blur(24px) saturate(180%);
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
        background: ${theme.bg};
        backdrop-filter: blur(32px) saturate(200%);
        -webkit-backdrop-filter: blur(32px) saturate(200%);
        border: 0.5px solid ${theme.border};
        border-radius: 16px;
        padding: 16px;
        padding-top: 20px;
        font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
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

// word selection handling
let inlinePopupContainer = null;

// update popup theme in real-time when theme changes
function updatePopupTheme() {
  if (!inlinePopupContainer) return;
  
  const popup = inlinePopupContainer.querySelector('.metldr-popup-body');
  if (!popup) return;
  
  // update popup background and border with !important
  popup.style.setProperty('background', currentTheme.bgSecondary, 'important');
  popup.style.setProperty('background-color', currentTheme.bgSecondary, 'important');
  popup.style.setProperty('border-color', currentTheme.border, 'important');
  popup.style.setProperty('box-shadow', `0 8px 24px ${currentTheme.shadow}, 0 4px 12px ${currentTheme.shadow}, inset 0 1px 0 ${currentTheme.borderSubtle}`, 'important');
  
  // update all text elements with !important
  const allSpans = popup.querySelectorAll('span');
  allSpans.forEach((span, index) => {
    if (index === 0) {
      // word (primary color)
      span.style.setProperty('color', currentTheme.primary, 'important');
    } else if (span.textContent.includes('AI')) {
      // ai badge (accent color)
      span.style.setProperty('color', currentTheme.accent, 'important');
    } else if (span.parentElement && (span.parentElement.tagName === 'HEADER' || span.style.fontSize === '10px')) {
      // part of speech (muted)
      span.style.setProperty('color', currentTheme.textMuted, 'important');
    }
  });
  
  // update POS tags specifically
  const posTags = popup.querySelectorAll('div[style*="font-size: 9px"][style*="text-transform: uppercase"]');
  posTags.forEach(tag => {
    tag.style.setProperty('color', currentTheme.secondary, 'important');
  });
  
  const content = popup.querySelector('.metldr-popup-content');
  if (content) {
    content.style.setProperty('color', currentTheme.text, 'important');
    const defDivs = content.querySelectorAll('div');
    defDivs.forEach(div => {
      div.style.setProperty('color', currentTheme.text, 'important');
    });
  }
  
  console.log('[POPUP] theme updated in real-time:', currentTheme);
}

function updateSummaryTheme() {
  console.log('updateSummaryTheme called with currentTheme:', currentTheme);

  const theme = currentTheme;
  console.log('Using theme object:', theme);

  const loadingElement = document.querySelector('.metldr-loading');
  if (loadingElement) {
    console.log('Updating loading element');
    loadingElement.style.setProperty('background', theme.bgSecondary, 'important');
    loadingElement.style.setProperty('background-color', theme.bgSecondary, 'important');
    loadingElement.style.setProperty('border-color', theme.border, 'important');
    loadingElement.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}`, 'important');

    const spinner = loadingElement.querySelector('.metldr-spinner');
    if (spinner) {
      console.log('Updating spinner');
      spinner.style.setProperty('border-color', theme.border, 'important');
      spinner.style.setProperty('border-top-color', theme.primary, 'important');
    }

    const loadingText = loadingElement.querySelector('span');
    if (loadingText) {
      console.log('Updating loading text');
      loadingText.style.setProperty('color', theme.text, 'important');
    }
  } else {
    console.log('No loading element found');
  }

  const existingSummary = document.querySelector('.metldr-summary');
  if (existingSummary) {
    console.log('Found existing summary, updating...');

    // update the status badge background and styling
    const statusBadge = existingSummary.querySelector('div[style*="position: absolute"]');
    if (statusBadge) {
      console.log('Updating status badge');
      statusBadge.style.setProperty('background', theme.bgSecondary, 'important');
      statusBadge.style.setProperty('background-color', theme.bgSecondary, 'important');
      statusBadge.style.setProperty('border-color', theme.borderSubtle, 'important');
      statusBadge.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');

      // update text colors in status badge
      const brandingText = statusBadge.querySelector('strong');
      if (brandingText) {
        console.log('Updating branding text');
        brandingText.style.setProperty('color', theme.primary, 'important');
      }

      const modelText = statusBadge.querySelector('span[title*="model used"]');
      if (modelText) {
        console.log('Updating model text');
        modelText.style.setProperty('color', theme.textMuted, 'important');
      }

      const timeText = statusBadge.querySelector('span[title*="time taken"]');
      if (timeText) {
        console.log('Updating time text');
        timeText.style.setProperty('color', theme.textMuted, 'important');
      }

      const separators = statusBadge.querySelectorAll('span[style*="opacity: 0.5"]');
      console.log(`Updating ${separators.length} separators`);
      separators.forEach(sep => sep.style.setProperty('color', theme.borderSubtle, 'important'));
    } else {
      console.log('Status badge not found');
    }

    // update regenerate button
    const regenerateBtn = existingSummary.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      console.log('Updating regenerate button');
      regenerateBtn.style.setProperty('background', theme.bgSecondary, 'important');
      regenerateBtn.style.setProperty('background-color', theme.bgSecondary, 'important');
      regenerateBtn.style.setProperty('border-color', theme.borderSubtle, 'important');
      regenerateBtn.style.setProperty('color', theme.primary, 'important');
      regenerateBtn.style.setProperty('box-shadow', `0 4px 12px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');
    } else {
      console.log('Regenerate button not found');
    }

    // update main card - use more specific selector for the outer card with backdrop-filter
    const mainCard = existingSummary.querySelector('div[style*="backdrop-filter: blur(32px)"]');
    if (mainCard) {
      console.log('updating main card');
      mainCard.style.setProperty('background', theme.bg, 'important');
      mainCard.style.setProperty('background-color', theme.bg, 'important');
      mainCard.style.setProperty('border-color', theme.border, 'important');
      mainCard.style.setProperty('box-shadow', `0 8px 32px ${theme.shadow}, inset 0 1px 0 ${theme.borderSubtle}`, 'important');

      // update summary items
      const summaryItems = existingSummary.querySelectorAll('.metldr-summary-item');
      console.log(`Found ${summaryItems.length} summary items to update`);
      summaryItems.forEach((item, index) => {
        console.log(`Updating summary item ${index + 1}`);
        // Set both background and background-color to ensure it overrides
        item.style.setProperty('background', theme.bgSecondary, 'important');
        item.style.setProperty('background-color', theme.bgSecondary, 'important');
        item.style.setProperty('color', theme.text, 'important');
        console.log(`   After: background=${item.style.background}, backgroundColor=${item.style.backgroundColor}`);

        // update action items header
        const actionHeader = item.querySelector('div[style*="text-transform: uppercase"]');
        if (actionHeader) {
          console.log(`Updating action header in item ${index + 1}`);
          actionHeader.style.setProperty('color', theme.secondary, 'important');
        }

        // update bullet points
        const bullets = item.querySelectorAll('span[style*="position: absolute"][style*="left: 0"]');
        console.log(`Updating ${bullets.length} bullets in item ${index + 1}`);
        bullets.forEach(bullet => {
          bullet.style.setProperty('color', theme.secondary, 'important');
        });

        // update list items
        const listItems = item.querySelectorAll('li');
        console.log(`Updating ${listItems.length} list items in item ${index + 1}`);
        listItems.forEach(li => li.style.setProperty('color', theme.text, 'important'));

        // Update date tags
        const dateTags = item.querySelectorAll('span[style*="background:"]');
        console.log(`Updating ${dateTags.length} date tags in item ${index + 1}`);
        dateTags.forEach(tag => {
          tag.style.setProperty('background', theme.bgSecondary, 'important');
          tag.style.setProperty('background-color', theme.bgSecondary, 'important');
          tag.style.setProperty('color', theme.accent, 'important');
        });
      });
    } else {
      console.log('Main card not found');
    }
  } else {
    console.log('No existing summary found');
  }

  console.log('updateSummaryTheme completed');
}document.addEventListener('mouseup', handleTextSelection);

function handleTextSelection(e) {
  // don't interfere with gmail email processing
  if (isGmail) return;
  
  // ignore mouseup inside existing popup (allow interaction within popup)
  if (inlinePopupContainer && inlinePopupContainer.contains(e.target)) {
    return;
  }
  
  const selection = window.getSelection();
  const selectedText = selection.toString().trim();
  
  // remove existing popup (with animation if present)
  if (inlinePopupContainer) {
    cleanupPopup();
  }
  
  if (!selectedText) return;
  
  const wordCount = selectedText.split(/\s+/).length;
  
  // single word: show inline popup for definition/translation
  if (wordCount === 1) {
    // get selection rect for accurate positioning
    const selection = window.getSelection();
    if (selection.rangeCount > 0) {
      const range = selection.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      showInlinePopup(selectedText, rect);
    }
  }
  // multi-word: context menu handles this (already implemented)
}

// store selection range for persistent positioning
let popupAnchorRange = null;
let scrollListener = null;
let resizeListener = null;
let clickListener = null;

// update popup position based on current word location
function updatePopupPosition() {
  if (!inlinePopupContainer || !popupAnchorRange) return;
  
  // get current position of the anchored word
  const rect = popupAnchorRange.getBoundingClientRect();
  const popupRect = inlinePopupContainer.getBoundingClientRect();
  
  const scrollX = window.pageXOffset || document.documentElement.scrollLeft;
  const scrollY = window.pageYOffset || document.documentElement.scrollTop;
  
  // calculate center of word in document coordinates
  const wordCenterX = rect.left + (rect.width / 2) + scrollX;
  const wordBottomY = rect.bottom + scrollY;
  
  // position popup centered below word
  let finalX = wordCenterX - (popupRect.width / 2);
  let finalY = wordBottomY + 12;
  
  // viewport boundary checks
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  
  // convert to viewport coordinates for bounds checking
  const viewportX = finalX - scrollX;
  const viewportY = finalY - scrollY;
  
  // clamp horizontal position within viewport
  if (viewportX < 10) {
    finalX = scrollX + 10;
  } else if (viewportX + popupRect.width > viewportWidth - 10) {
    finalX = scrollX + viewportWidth - popupRect.width - 10;
  }
  
  // flip above word if no space below
  if (viewportY + popupRect.height > viewportHeight - 10) {
    finalY = wordBottomY - rect.height - popupRect.height - 8;
  }
  
  // ensure not off top of viewport
  if ((finalY - scrollY) < 10) {
    finalY = scrollY + 10;
  }
  
  // apply position
  inlinePopupContainer.style.top = finalY + 'px';
  inlinePopupContainer.style.left = finalX + 'px';
}

// cleanup popup and all event listeners
function cleanupPopup() {
  if (!inlinePopupContainer) return;
  
  // smooth fade-out animation before removal
  gsap.to(inlinePopupContainer, {
    opacity: 0,
    scale: 0.94,
    y: -4,
    duration: 0.12,
    ease: 'power2.in',
    onComplete: () => {
      if (inlinePopupContainer) {
        inlinePopupContainer.remove();
        inlinePopupContainer = null;
        popupAnchorRange = null;
      }
    }
  });
  
  // remove event listeners immediately (don't wait for animation)
  if (scrollListener) {
    window.removeEventListener('scroll', scrollListener, true);
    scrollListener = null;
  }
  
  if (resizeListener) {
    window.removeEventListener('resize', resizeListener);
    resizeListener = null;
  }
  
  if (clickListener) {
    document.removeEventListener('click', clickListener);
    clickListener = null;
  }
}

async function showInlinePopup(word, selectionRect) {
  const settings = await chrome.storage.local.get(['wordPopupEnabled']);
  if (settings.wordPopupEnabled === false) return;
  
  // reload current theme to ensure reactivity
  await loadCurrentTheme();
  
  const isEnglish = /^[a-zA-Z]+$/.test(word);
  const lookupType = isEnglish ? 'definition' : 'translation';
  
  // store the selection range for continuous repositioning
  const selection = window.getSelection();
  if (selection.rangeCount > 0) {
    popupAnchorRange = selection.getRangeAt(0).cloneRange();
  }
  
  // create popup with absolute positioning
  inlinePopupContainer = document.createElement('div');
  inlinePopupContainer.className = 'metldr-inline-word-popup';
  
  // initial position (will be updated by updatePopupPosition)
  inlinePopupContainer.style.cssText = `
    position: absolute;
    top: 0px;
    left: 0px;
    z-index: 999999;
    transform-origin: top left;
    visibility: hidden;
    will-change: opacity, transform;
  `;
  
  const popup = document.createElement('div');
  popup.className = 'metldr-popup-body';
  popup.style.cssText = `
    --metldr-primary: ${currentTheme.primary};
    background: ${currentTheme.bgSecondary};
    border: 1.5px solid ${currentTheme.border};
    border-radius: 12px;
    padding: 12px 16px;
    min-width: 240px;
    max-width: 360px;
    box-shadow: 0 8px 24px ${currentTheme.shadow}, 0 4px 12px ${currentTheme.shadow}, inset 0 1px 0 ${currentTheme.borderSubtle};
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
    will-change: opacity, transform;
  `;
  
  // header: word + part of speech + source (single line)
  const header = document.createElement('div');
  header.style.cssText = `
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
    flex-wrap: wrap;
  `;
  
  const wordSpan = document.createElement('span');
  wordSpan.style.cssText = `
    font-size: 15px;
    font-weight: 700;
    color: ${currentTheme.primary};
    letter-spacing: 0.01em;
    line-height: 1.4;
  `;
  wordSpan.textContent = word;
  header.appendChild(wordSpan);
  
  // content area (definition)
  const content = document.createElement('div');
  content.className = 'metldr-popup-content';
  content.style.cssText = `
    font-size: 13px;
    color: ${currentTheme.text};
    line-height: 1.6;
    min-height: 20px;
    font-weight: 400;
    -webkit-font-smoothing: antialiased;
  `;
  
  // loading state
  const loader = document.createElement('div');
  loader.style.cssText = 'display: flex; align-items: center; gap: 6px;';
  
  const spinner = document.createElement('div');
  spinner.style.cssText = `
    width: 12px;
    height: 12px;
    border: 2.5px solid ${currentTheme.border};
    border-top-color: ${currentTheme.primary};
    border-radius: 50%;
    animation: metldr-spin 0.6s linear infinite;
  `;
  
  const loadText = document.createElement('span');
  loadText.textContent = 'looking up...';
  loadText.style.cssText = `
    color: ${currentTheme.textMuted};
    font-size: 11px;
    font-weight: 500;
  `;
  
  loader.appendChild(spinner);
  loader.appendChild(loadText);
  
  // add loading to header
  const tempMeta = document.createElement('span');
  tempMeta.style.cssText = `
    font-size: 10px;
    color: ${currentTheme.textMuted};
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  `;
  tempMeta.textContent = '...';
  header.appendChild(tempMeta);
  
  content.appendChild(loader);
  
  popup.appendChild(header);
  popup.appendChild(content);
  inlinePopupContainer.appendChild(popup);
  document.body.appendChild(inlinePopupContainer);
  
  // position popup initially
  updatePopupPosition();
  
  // show popup with animation
  inlinePopupContainer.style.visibility = 'visible';
  
  // smooth pop animation with GSAP
  gsap.fromTo(inlinePopupContainer, 
    { 
      opacity: 0, 
      scale: 0.92, 
      y: -6 
    }, 
    { 
      opacity: 1, 
      scale: 1, 
      y: 0, 
      duration: 0.16, 
      ease: 'back.out(1.7)' 
    }
  );
  
  // setup event listeners for persistent positioning
  scrollListener = () => updatePopupPosition();
  resizeListener = () => updatePopupPosition();
  clickListener = (e) => {
    // don't close if clicking inside popup
    if (inlinePopupContainer && !inlinePopupContainer.contains(e.target)) {
      cleanupPopup();
    }
  };
  
  // reposition on scroll/resize, close on outside click
  window.addEventListener('scroll', scrollListener, true);
  window.addEventListener('resize', resizeListener);
  
  // add click listener after small delay to avoid immediate closure
  setTimeout(() => {
    document.addEventListener('click', clickListener);
  }, 100);
  
  // fetch result
  try {
    console.log('[POPUP] sending word lookup request:', word);
    const response = await chrome.runtime.sendMessage({
      type: 'WORD_LOOKUP',
      word,
      lookupType
    });
    
    console.log('[POPUP] received response:', response);
    
    // clear loading state
    header.innerHTML = '';
    content.innerHTML = '';
    
    // rebuild header with word
    const wordSpan = document.createElement('span');
    wordSpan.style.cssText = `
      font-size: 14px;
      font-weight: 600;
      color: ${currentTheme.primary || '#00f0ff'};
      letter-spacing: 0.2px;
    `;
    wordSpan.textContent = word;
    header.appendChild(wordSpan);
    
    if (!response || response.error) {
      const errorText = document.createElement('span');
      errorText.textContent = response?.error || 'lookup failed';
      errorText.style.cssText = `
        color: ${currentTheme.secondary || '#ff0080'};
        font-size: 11px;
      `;
      content.appendChild(errorText);
    } else if (response.result) {
      if (lookupType === 'definition') {
        // add source indicator for ollama
        if (response.result.source === 'ollama') {
          const sourceHint = document.createElement('span');
          sourceHint.style.cssText = `
            font-size: 9px;
            color: ${currentTheme.yellow || '#fbbf24'};
            margin-left: 8px;
            opacity: 0.8;
            letter-spacing: 0.3px;
          `;
          sourceHint.textContent = 'AI';
          header.appendChild(sourceHint);
        }
        
        // render all definitions
        const definitions = response.result.definitions || [];
        
        if (definitions.length === 0) {
          const errorText = document.createElement('span');
          errorText.textContent = 'no definition found';
          errorText.style.cssText = `
            color: ${currentTheme.textMuted || '#888'};
            font-size: 11px;
          `;
          content.appendChild(errorText);
        } else {
          // create scrollable container for definitions
          const defsContainer = document.createElement('div');
          defsContainer.className = 'metldr-definitions-scroll';
          defsContainer.style.cssText = `
            max-height: 280px;
            overflow-y: auto;
            margin: 0;
            padding-right: 4px;
          `;
          
          // prevent internal scroll from triggering popup repositioning
          defsContainer.addEventListener('scroll', (e) => {
            e.stopPropagation();
          }, true);
          
          definitions.forEach((def, index) => {
            const defBlock = document.createElement('div');
            defBlock.style.cssText = `
              margin-bottom: ${index < definitions.length - 1 ? '12px' : '0'};
              padding-bottom: ${index < definitions.length - 1 ? '12px' : '0'};
              border-bottom: ${index < definitions.length - 1 ? `1px solid ${currentTheme.border || 'rgba(255,255,255,0.1)'}` : 'none'};
            `;
            
            // part of speech tag
            const posTag = document.createElement('div');
            posTag.style.cssText = `
              font-size: 9px;
              color: ${currentTheme.secondary || '#ff0080'};
              text-transform: uppercase;
              letter-spacing: 0.5px;
              font-weight: 600;
              margin-bottom: 4px;
            `;
            posTag.textContent = def.partOfSpeech || 'unknown';
            defBlock.appendChild(posTag);
            
            // definition text
            const defText = document.createElement('div');
            defText.style.cssText = `
              font-size: 13px;
              line-height: 1.55;
              color: ${currentTheme.text || '#e0e0e0'};
              margin: 0;
            `;
            defText.textContent = def.definition;
            defBlock.appendChild(defText);
            
            // example (if available)
            if (def.example) {
              const exampleText = document.createElement('div');
              exampleText.style.cssText = `
                font-size: 11px;
                line-height: 1.5;
                color: ${currentTheme.textMuted || '#888'};
                margin-top: 4px;
                font-style: italic;
              `;
              exampleText.textContent = `"${def.example}"`;
              defBlock.appendChild(exampleText);
            }
            
            defsContainer.appendChild(defBlock);
          });
          
          content.appendChild(defsContainer);
        }
      } else {
        const langInfo = document.createElement('p');
        langInfo.textContent = `${response.result.sourceLang || 'unknown'} → ${response.result.targetLang || 'english'}`;
        langInfo.style.cssText = `
          margin: 0 0 6px 0;
          font-size: 9px;
          color: ${currentTheme.textMuted || '#888'};
          text-transform: uppercase;
        `;
        
        const trans = document.createElement('p');
        trans.textContent = response.result.translation || 'translation unavailable';
        trans.style.cssText = `
          margin: 0;
          font-weight: 600;
          color: ${currentTheme.primary || '#00f0ff'};
        `;
        
        content.appendChild(langInfo);
        content.appendChild(trans);
      }
    }
    
    // reposition after content loaded (popup size changed)
    updatePopupPosition();
  } catch (error) {
    console.error('metldr: word lookup failed:', error);
    content.innerHTML = '';
    const errorText = document.createElement('span');
    errorText.textContent = 'lookup failed';
    errorText.style.cssText = `color: ${currentTheme.secondary || '#ff0080'}; font-size: 10px;`;
    content.appendChild(errorText);
    
    // reposition after error content loaded
    updatePopupPosition();
  }
}

// add animations and scrollbar styling
if (!document.getElementById('metldr-word-popup-animations')) {
  const style = document.createElement('style');
  style.id = 'metldr-word-popup-animations';
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
    
    /* minimal themed scrollbar */
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
    
    /* firefox scrollbar */
    .metldr-definitions-scroll {
      scrollbar-width: thin;
      scrollbar-color: var(--metldr-primary) transparent;
    }
  `;
  document.head.appendChild(style);
}

// dwell-time monitoring for pre-summarisation
function startDwellMonitoring() {
  console.log('metldr: starting dwell-time monitoring');
  
  // reset timer on page change
  const urlObserver = setInterval(() => {
    if (window.location.href !== currentPageUrl) {
      console.log('metldr: url changed, resetting dwell timer');
      dwellTimer = 0;
      summarisationQueued = false;
      currentPageUrl = window.location.href;
    }
  }, 1000);

  // increment dwell timer when page is focused
  dwellInterval = setInterval(() => {
    if (!document.hidden && document.hasFocus()) {
      dwellTimer++;
      
      // trigger pre summarisation at threshold
      if (dwellTimer === DWELL_THRESHOLD && !summarisationQueued) {
        console.log('metldr: dwell threshold reached, queueing pre-summarisation');
        queuePreSummarisation();
      }
    }
  }, 1000);

  // reset timer on user navigation
  window.addEventListener('beforeunload', () => {
    dwellTimer = 0;
    summarisationQueued = false;
  });

  // pause timer when tab hidden
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      console.log('metldr: tab hidden, pausing dwell timer');
    } else {
      console.log('metldr: tab visible, resuming dwell timer');
    }
  });
}

async function queuePreSummarisation() {
  summarisationQueued = true;
  
  // detect if page is worth summarising
  const detector = new ContentDetector();
  const pageInfo = detector.detectPageType();
  
  console.log('metldr: page type detected:', pageInfo.type, 'confidence:', pageInfo.confidence);
  
  if (!detector.isReadable()) {
    console.log('metldr: page not readable, skipping pre-summarisation');
    return;
  }

  // extract content
  const extracted = detector.extractContent();
  
  if (!extracted.content || extracted.content.length < 200) {
    console.log('metldr: insufficient content, skipping');
    return;
  }

  console.log('metldr: sending content for pre-summarisation');
  
  // send to background for low priority summarisation
  chrome.runtime.sendMessage({
    type: 'PRE_SUMMARISE',
    priority: 'low',
    url: window.location.href,
    pageType: pageInfo.type,
    metadata: pageInfo.metadata,
    content: extracted.content.slice(0, 5000), // limit to 5k chars
    sections: extracted.sections
  }).catch(err => {
    console.error('metldr: failed to queue pre-summarisation:', err);
  });
}
