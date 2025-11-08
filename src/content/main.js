// metldr gmail integration

console.log('MeTLDR: Content script loaded');
console.log('MeTLDR: Current URL:', window.location.href);
console.log('MeTLDR: Hostname:', window.location.hostname);

const isGmail = window.location.hostname.includes('mail.google.com');
console.log('MeTLDR: Is Gmail?', isGmail);

// track processed emails globally
const processedEmails = new Set();

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
  
  // wait for gmail to be ready
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

// inject summary UI into gmail
function initInjector() {
  console.log('MeTLDR: initInjector called');
  let lastProcessedUrl = '';

  // function to process emails
  const processEmails = () => {
    const currentUrl = window.location.href;
    console.log('MeTLDR: Mutation detected, URL:', currentUrl);
    
    // check if url changed (user opened new email)
    if (currentUrl === lastProcessedUrl) {
      console.log('MeTLDR: Same URL, skipping');
      return;
    }
    
    lastProcessedUrl = currentUrl;
    const threadId = getCurrentThreadId();
    console.log('MeTLDR: Current thread ID:', threadId);
    
    if (threadId && !processedEmails.has(threadId)) {
      processedEmails.add(threadId);
      console.log('MeTLDR: New thread detected, processing in 1.5s...');
      setTimeout(() => processCurrentEmail(), 1500);
    } else if (processedEmails.has(threadId)) {
      console.log('MeTLDR: Thread already processed');
    }
  };

  // observe gmail's DOM for email thread changes
  const observer = new MutationObserver(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 300);
  });

  // observe the main gmail container
  const mainContainer = document.querySelector('div[role="main"]') || document.body;
  console.log('MeTLDR: Main container found?', !!mainContainer);
  
  if (mainContainer) {
    observer.observe(mainContainer, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class']
    });
    console.log('MeTLDR: Observer started');
  }

  // scan on initial load
  console.log('MeTLDR: Will scan for emails in 2s...');
  setTimeout(() => {
    console.log('MeTLDR: Scanning for initial email...');
    lastProcessedUrl = window.location.href;
    processCurrentEmail();
  }, 2000);

  // store observer for cleanup
  initInjector.observer = observer;
  console.log('MeTLDR: initInjector complete');

  // hook url changes (gmail spa navigation)
  hookUrlChanges(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 200);
  });
}

// get current open email thread ID
function getCurrentThreadId() {
  const url = window.location.href;
  console.log('MeTLDR: Checking URL for thread ID:', url);
  
  // try different gmail url patterns
  let match = url.match(/#inbox\/([a-zA-Z0-9_-]+)/); // #inbox/THREAD_ID
  if (match) {
    console.log('MeTLDR: Found thread ID:', match[1]);
    return match[1];
  }
  
  match = url.match(/\/mail\/u\/\d+\/.*#inbox\/([a-zA-Z0-9_-]+)/);
  if (match) {
    console.log('MeTLDR: Found thread ID (alt):', match[1]);
    return match[1];
  }
  
  console.log('MeTLDR: No thread ID found in URL');
  return null;
}

// observe rmail SPA url changes
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

    // fallback poller (handles edge cases)
    let last = location.href;
    setInterval(() => {
      if (location.href !== last) { last = location.href; fire(); }
    }, 1000);

    console.log('MeTLDR: URL change hooks installed');
  } catch (e) {
    console.error('MeTLDR: Failed to install URL hooks', e);
  }
}

// process currently visible email
async function processCurrentEmail() {
  console.log('MeTLDR: Processing current email...');
  
  // only process if we have a thread ID (i.e., inside a specific email)
  const threadId = getCurrentThreadId();
  if (!threadId) {
    console.log('MeTLDR: No thread ID, skipping (inbox view)');
    return;
  }
  
  // check if already processed to avoid duplicates
  const existingSummary = document.querySelector('.metldr-summary');
  if (existingSummary) {
    console.log('MeTLDR: Summary already displayed for this email');
    return;
  }
  
  // check if this email thread was already processed (even if summary not visible yet)
  if (processedEmails.has(threadId)) {
    console.log('MeTLDR: This thread already being processed:', threadId);
    // still proceed to show cached summary if available
  }
  
  // find email container
  const emailContainer = document.querySelector('.nH.if') || 
                          document.querySelector('[data-thread-id]') ||
                          document.querySelector('.gs') ||
                          document.querySelector('div[role="main"]');
  
  console.log('MeTLDR: Email container found?', !!emailContainer);
  
  if (!emailContainer) {
    console.log('MeTLDR: No email container found');
    return;
  }
  
  console.log('MeTLDR: Processing current email thread...');
  await processEmailThread(emailContainer);
}

// process a single email thread
async function processEmailThread(threadElement) {
  try {
    // extract email text
    const emailText = extractEmailText(threadElement);
    console.log('MeTLDR: Extracted email text length:', emailText?.length);
    
    if (!emailText || emailText.length < 50) {
      console.log('MeTLDR: Email text too short or empty');
        return;
      }
      
    console.log('MeTLDR: Processing email...');

    // show loading animation
    const loadingDiv = showLoading(threadElement);

    // get summary from background script using thread ID for caching
    const threadId = getCurrentThreadId();
    const summary = await getSummaryFromBackground(emailText, threadId);
    console.log('MeTLDR: Received summary:', summary);

    // remove loading
    if (loadingDiv) loadingDiv.remove();

    // inject summary
    if (summary) {
      injectSummaryUI(threadElement, summary);
    }

  } catch (error) {
    console.error('MeTLDR: Failed to process:', error);
  }
}

// extract email text from gmail DOM
function extractEmailText(threadElement) {
  console.log('MeTLDR: Extracting email text...');
  
  // get the email content area
  const selectors = [
    '.ii.gt', // email body class
    '.a3s.aiL', //  Gmail body
    '.ii', //  email content
    '[dir="ltr"]', // emails
    '.gmail_signature' // before signature
  ];

  let fullText = '';
  
  // find ALL email messages in the thread, not just one
  for (const selector of selectors) {
    const elements = document.querySelectorAll(selector);
    console.log(`MeTLDR: Found ${elements.length} elements with selector ${selector}`);
    
    if (elements.length > 0) {
      // collect ALL text from ALL matching elements
      for (const element of elements) {
        const text = element.textContent?.trim();
        if (text && text.length > 20 && !text.match(/^[\s\n]*$/)) {
          fullText += text + '\n\n---\n\n';
        }
      }
      
      // continue to next selector for more content
    }
  }
  
  // return the accumulated text
  if (fullText.length > 50) {
    console.log('MeTLDR: Extracted full email text, length:', fullText.length);
    return fullText;
  }

  // if nothing found, try getting ALL text from the visible area
  const allVisibleText = document.body.textContent;
  if (allVisibleText && allVisibleText.length > 500) {
    console.log('MeTLDR: Using full page text, length:', allVisibleText.length);
    return allVisibleText;
  }

  console.warn('MeTLDR: Could not extract email text properly');
  return '';
}

// get summary via background script
async function getSummaryFromBackground(emailText, emailId, forceRegenerate = false) {
  return new Promise((resolve) => {
    if (!chrome || !chrome.runtime || !chrome.runtime.sendMessage) {
      console.error('MeTLDR: Chrome runtime not available - extension may need reload');
      resolve(null);
      return;
    }

    try {
      chrome.runtime.sendMessage({
        type: 'SUMMARIZE_EMAIL',
        emailContent: emailText,
        emailId: emailId,
        forceRegenerate: forceRegenerate
      }, (response) => {
        if (chrome.runtime.lastError) {
          console.error('MeTLDR: Background error:', chrome.runtime.lastError);
          resolve(null);
        } else {
          resolve(response?.summary || null);
        }
      });
    } catch (error) {
      console.error('MeTLDR: Failed to send message:', error);
      resolve(null);
    }
  });
}

// show loading animation matching current theme
function showLoading(threadElement) {
  const theme = THEME_COLORS[currentTheme] || THEME_COLORS.cyberpunk;
  
  const loading = document.createElement('div');
  loading.className = 'metldr-loading';
  loading.innerHTML = `
    <div style="
      padding: 10px 14px;
      background: linear-gradient(135deg, ${theme.bg}, ${theme.bgSecondary});
      border: 1px solid ${theme.border};
      border-radius: 16px;
      margin: 12px 0;
      box-shadow: 0 4px 24px rgba(0,0,0,0.15), 0 2px 8px rgba(0,0,0,0.1), inset 0 1px 0 ${theme.border}40;
      backdrop-filter: blur(20px) saturate(130%);
      -webkit-backdrop-filter: blur(20px) saturate(130%);
      animation: fadeInUp 0.3s ease-out;
      -webkit-font-smoothing: antialiased;
    ">
      <div style="
        display: flex;
        align-items: center;
        gap: 10px;
      ">
        <div style="
          width: 16px;
          height: 16px;
          border: 2px solid ${theme.border};
          border-top-color: ${theme.primary};
          border-radius: 50%;
          animation: spin 0.6s linear infinite;
        "></div>
        <span style="
          color: ${theme.text};
          font-size: 13px;
          font-weight: 600;
          -webkit-font-smoothing: antialiased;
        ">Generating summary...</span>
      </div>
    </div>
    <style>
      @keyframes spin {
        to { transform: rotate(360deg); }
      }
      @keyframes fadeInUp {
        from {
          opacity: 0;
          transform: translateY(-10px);
        }
        to {
          opacity: 1;
          transform: translateY(0);
        }
      }
    </style>
  `;

  // find the email header to insert AFTER it
  const emailHeader = document.querySelector('.gE.iv.gt') || 
                       document.querySelector('.gE.iv') ||
                       document.querySelector('[data-message-id]');
  
  if (emailHeader && emailHeader.parentNode) {
    emailHeader.parentNode.insertBefore(loading, emailHeader.nextSibling);
  } else {
    console.error('MeTLDR: Could not find email header for loading indicator');
  }

  return loading;
}

// theme
const THEME_COLORS = {
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
  },
};

// current theme state (from chrome.storage)
let currentTheme = 'cyberpunk';

// load theme from chrome.storage
async function loadCurrentTheme() {
  try {
    const result = await chrome.storage.local.get('theme');
    currentTheme = result.theme || 'cyberpunk';
    console.log('MeTLDR: Loaded theme from storage:', currentTheme);
  } catch (error) {
    console.error('MeTLDR: Failed to load theme:', error);
    currentTheme = 'cyberpunk';
  }
}

// listen for theme changes from side panel
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && changes.theme) {
    const newTheme = changes.theme.newValue;
    console.log('MeTLDR: Theme changed to:', newTheme);
    currentTheme = newTheme;
    
    // update existing summary if visible
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
    console.log('MeTLDR: Summary already exists, skipping injection');
    return;
  }
  
  console.log('MeTLDR: Injecting summary UI with theme:', currentTheme);

  const summaryText = summary.summary || '';
  const actions = summary.action_items || [];
  const dates = summary.dates || [];
  const confidence = summary.confidence || 'medium';
  const currentThreadId = getCurrentThreadId();
  const theme = THEME_COLORS[currentTheme] || THEME_COLORS.cyberpunk;

  const confidenceColors = {
    high: '#10b981',
    medium: '#f59e0b',
    low: '#ef4444'
  };
  const confColor = confidenceColors[confidence] || '#6b7280';

  // create card
  const summaryDiv = document.createElement('div');
  summaryDiv.className = 'metldr-summary';
  summaryDiv.style.cssText = 'opacity: 0; transform: translateY(-10px) scale(0.98);';
  
  summaryDiv.innerHTML = `
    <div style="
      background: linear-gradient(135deg, ${theme.bg}, ${theme.bgSecondary});
      border: 1px solid ${theme.border};
      border-radius: 16px;
      padding: 10px 14px;
      margin: 12px 0;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
      box-shadow: 0 4px 24px rgba(0,0,0,0.15), 0 2px 8px rgba(0,0,0,0.1), inset 0 1px 0 ${theme.border}40;
      backdrop-filter: blur(20px) saturate(130%);
      -webkit-backdrop-filter: blur(20px) saturate(130%);
      position: relative;
      overflow: hidden;
      -webkit-font-smoothing: antialiased;
      -moz-osx-font-smoothing: grayscale;
      transform: translateZ(0);
      will-change: transform;
    ">
      <div style="
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: ${summaryText || actions.length > 0 ? '8px' : '0'};
        gap: 8px;
      ">
        <div style="display: flex; align-items: center; gap: 6px;">
          <div style="
            width: 7px;
            height: 7px;
            background: ${confColor};
            border-radius: 50%;
            box-shadow: 0 0 8px ${confColor}, 0 0 12px ${confColor}80;
            animation: statusPulse 2s ease-in-out infinite;
          "></div>
          <strong style="
            font-size: 13px;
            font-weight: 600;
            color: ${theme.primary};
            letter-spacing: -0.01em;
            -webkit-font-smoothing: antialiased;
          ">MeTLDR</strong>
          <span title="confidence level: how certain the AI is about this summary's accuracy" style="
            font-size: 11px;
            color: ${theme.text};
            padding: 2px 7px;
            background: ${theme.bgSecondary};
            border-radius: 8px;
            font-weight: 600;
            letter-spacing: 0.02em;
            -webkit-font-smoothing: antialiased;
            cursor: help;
          ">${confidence}</span>
        </div>
        <button class="metldr-regenerate-btn" data-thread-id="${currentThreadId}" title="regenerate summary with fresh AI analysis" style="
          padding: 4px 10px;
          background: ${theme.bgSecondary};
          border: 1px solid ${theme.border};
          border-radius: 8px;
          color: ${theme.primary};
          font-size: 11px;
          font-weight: 600;
          cursor: pointer;
          transition: all 0.15s cubic-bezier(0.4, 0, 0.2, 1);
          -webkit-font-smoothing: antialiased;
        ">↻</button>
      </div>

      ${summaryText ? `
        <div class="metldr-summary-item" style="
          background: ${theme.bgSecondary};
          border-radius: 10px;
          padding: 10px 12px;
          margin-bottom: ${actions.length > 0 || dates.length > 0 ? '6px' : '0'};
          font-size: 14px;
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
          padding: 10px 12px;
          background: ${theme.bgSecondary};
          border-radius: 10px;
        ">
          <div style="
            font-size: 11px;
            color: ${theme.secondary};
            text-transform: uppercase;
            letter-spacing: 0.05em;
            font-weight: 700;
            margin-bottom: 6px;
            opacity: 0.8;
          ">Actions</div>
          <ul style="
            margin: 0;
            padding-left: 0;
            list-style: none;
            color: ${theme.text};
            line-height: 1.4;
            font-size: 13px;
          ">
            ${actions.map(action => `
              <li style="
                padding-left: 14px;
                position: relative;
                margin-bottom: 4px;
                font-weight: 400;
              ">
                <span style="
                  position: absolute;
                  left: 0;
                  top: 2px;
                  color: ${theme.secondary};
                  font-size: 10px;
                ">•</span>
                ${escapeHtml(action)}
              </li>
            `).join('')}
          </ul>
        </div>
      ` : ''}
        
      ${dates.length > 0 ? `
        <div class="metldr-summary-item" style="
          font-size: 12px;
          color: ${theme.accent};
          display: flex;
          flex-wrap: wrap;
          align-items: center;
          gap: 4px;
          font-weight: 600;
          -webkit-font-smoothing: antialiased;
        ">
          ${dates.map(d => `<span style="background: ${theme.bgSecondary}; padding: 3px 8px; border-radius: 6px; font-size: 11px; -webkit-font-smoothing: antialiased;">${escapeHtml(d)}</span>`).join('')}
        </div>
      ` : ''}
        
      ${summary.time_ms ? `
        <div style="
          font-size: 11px;
          color: ${theme.textMuted};
          opacity: 0.85;
          margin-top: 8px;
          letter-spacing: 0.01em;
          font-weight: 500;
          -webkit-font-smoothing: antialiased;
        ">
          <span style="opacity: 0.7; font-weight: 400;">Time taken:</span> ${formatTime(summary.time_ms)}${summary.cached ? ' • cached' : ''}
        </div>
      ` : ''}
    </div>
    
    <style>
      .metldr-regenerate-btn:hover {
        background: ${theme.primary}20 !important;
        border-color: ${theme.primary}60 !important;
        transform: scale(1.05);
      }
      .metldr-regenerate-btn:active {
        transform: scale(0.95);
      }
    </style>
  `;

  // inject into Gmail right after sender info (same as loading indicator)
  const emailHeader = document.querySelector('.gE.iv.gt') || 
                       document.querySelector('.gE.iv') ||
                       document.querySelector('[data-message-id]');
  
  if (emailHeader && emailHeader.parentNode) {
    emailHeader.parentNode.insertBefore(summaryDiv, emailHeader.nextSibling);
    console.log('MeTLDR: Summary injected, animating...');
    
    setTimeout(() => {
      summaryDiv.style.transition = 'all 0.6s cubic-bezier(0.16, 1, 0.3, 1)';
      summaryDiv.style.opacity = '1';
      summaryDiv.style.transform = 'translateY(0) scale(1)';
      
      // stagger items
      const items = summaryDiv.querySelectorAll('.metldr-summary-item');
      items.forEach((item, i) => {
        item.style.opacity = '0';
        item.style.transform = 'translateY(10px)';
        setTimeout(() => {
          item.style.transition = 'all 0.4s cubic-bezier(0.16, 1, 0.3, 1)';
          item.style.opacity = '1';
          item.style.transform = 'translateY(0)';
        }, 300 + (i * 80));
      });
    }, 50);
    
    const regenerateBtn = summaryDiv.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        const threadId = regenerateBtn.getAttribute('data-thread-id');
        console.log('MeTLDR: Regenerate button clicked for:', threadId);
        await regenerateSummary(threadId);
      });
      
      regenerateBtn.addEventListener('mouseenter', () => {
        regenerateBtn.style.transform = 'scale(1.05)';
      });
      regenerateBtn.addEventListener('mouseleave', () => {
        regenerateBtn.style.transform = 'scale(1)';
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

// regenerate summary (called by button click)
async function regenerateSummary(threadId) {
  console.log('MeTLDR: Regenerating summary for:', threadId);
  
  // remove existing summary
  const existing = document.querySelector('.metldr-summary');
  if (existing) existing.remove();
  
  // process email again with forceRegenerate flag
  const emailText = extractEmailText(document);
  if (emailText && emailText.length > 50) {
    const emailContainer = document.querySelector('.nH.if') || 
                          document.querySelector('[data-thread-id]') ||
                          document.querySelector('.gs');
    
    if (emailContainer) {
      const loadingDiv = showLoading(emailContainer);
      const summary = await getSummaryFromBackground(emailText, threadId, true);
      if (loadingDiv) loadingDiv.remove();
      
      if (summary) {
        injectSummaryUI(emailContainer, summary);
      }
    }
  }
}
