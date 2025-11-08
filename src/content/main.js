// MeTLDR Gmail Integration
// Seamless email summaries directly in Gmail with smooth animations

console.log('MeTLDR: Content script loaded');
console.log('MeTLDR: Current URL:', window.location.href);
console.log('MeTLDR: Hostname:', window.location.hostname);

const isGmail = window.location.hostname.includes('mail.google.com');
console.log('MeTLDR: Is Gmail?', isGmail);

// track processed emails globally
const processedEmails = new Set();

if (isGmail) {
  console.log('MeTLDR: Gmail detected');
  
  // wait for Gmail to be ready
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

// inject summary UI into Gmail
function initInjector() {
  console.log('MeTLDR: initInjector called');
  let lastProcessedUrl = '';

  // function to process emails
  const processEmails = () => {
    const currentUrl = window.location.href;
    console.log('MeTLDR: Mutation detected, URL:', currentUrl);
    
    // check if URL changed (user opened new email)
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

  // observe Gmail's DOM for email thread changes
  const observer = new MutationObserver(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 300);
  });

  // observe the main Gmail container
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

  // hook URL changes (Gmail SPA navigation)
  hookUrlChanges(() => {
    clearTimeout(initInjector.debounceTimer);
    initInjector.debounceTimer = setTimeout(processEmails, 200);
  });
}

// get current open email thread ID
function getCurrentThreadId() {
  const url = window.location.href;
  console.log('MeTLDR: Checking URL for thread ID:', url);
  
  // try different Gmail URL patterns
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

// observe Gmail SPA URL changes
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

// extract email text from Gmail DOM
function extractEmailText(threadElement) {
  console.log('MeTLDR: Extracting email text...');
  
  // get the email content area (the actual email message body)
  const selectors = [
    '.ii.gt', // email body class
    '.a3s.aiL', // modern Gmail body
    '.ii', // general email content
    '[dir="ltr"]', // emails typically have this
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
    // Check if chrome.runtime is available (extension context valid)
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

// show loading animation with Material You style
function showLoading(threadElement) {
  const loading = document.createElement('div');
  loading.className = 'metldr-loading';
  loading.innerHTML = `
    <div style="
      padding: 12px 16px;
      background: rgba(241, 243, 244, 0.8);
      backdrop-filter: blur(10px);
      border-radius: 12px;
      margin: 12px 0;
      box-shadow: 0 1px 3px rgba(0,0,0,0.1);
      animation: fadeInUp 0.3s ease-out;
    ">
      <div style="
        display: flex;
        align-items: center;
        gap: 12px;
      ">
        <div style="
          width: 20px;
          height: 20px;
          border: 3px solid rgba(26, 115, 232, 0.2);
          border-top-color: rgb(26, 115, 232);
          border-radius: 50%;
          animation: spin 0.8s linear infinite;
        "></div>
        <span style="
          color: rgb(60, 64, 67);
          font-size: 0.925em;
          font-weight: 500;
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

// inject beautiful summary UI with Material You style
function injectSummaryUI(threadElement, summary) {
  // check if already injected (prevent duplicates)
  const existingSummary = threadElement.querySelector('.metldr-summary') || 
                          document.querySelector('.metldr-summary');
  if (existingSummary) {
    console.log('MeTLDR: Summary already exists, skipping injection');
    return;
  }
  
  console.log('MeTLDR: Injecting summary UI...');

  const summaryText = summary.summary || '';
  const actions = summary.action_items || [];
  const dates = summary.dates || [];
  
  // get current thread ID for regenerate button
  const currentThreadId = getCurrentThreadId();

  // create modern Material You glossy card
  const summaryDiv = document.createElement('div');
  summaryDiv.className = 'metldr-summary';
  summaryDiv.innerHTML = `
    <div style="
      background: linear-gradient(135deg, rgba(241, 243, 244, 0.9) 0%, rgba(255, 255, 255, 0.95) 100%);
      backdrop-filter: blur(20px);
      border: 1px solid rgba(26, 115, 232, 0.2);
      border-radius: 16px;
      margin: 12px 0;
      position: relative;
      box-shadow: 0 2px 8px rgba(0,0,0,0.08), 0 1px 2px rgba(0,0,0,0.04);
      animation: slideInUp 0.4s cubic-bezier(0.16, 1, 0.3, 1);
      transition: all 0.2s ease;
    ">
      <div style="
        position: absolute;
        top: -10px;
        left: 12px;
        background: linear-gradient(135deg, rgb(26, 115, 232) 0%, rgb(11, 87, 208) 100%);
        color: white;
        font-size: 0.7em;
        padding: 4px 10px;
        border-radius: 6px;
        font-weight: 600;
        box-shadow: 0 2px 4px rgba(26, 115, 232, 0.3);
        z-index: 1;
      ">MeTLDR</div>
      
      <button class="metldr-regenerate-btn" data-thread-id="${currentThreadId}" style="
        position: absolute;
        top: -10px;
        right: 12px;
        padding: 4px 8px;
        background: rgba(255, 255, 255, 0.95);
        border: 1px solid rgba(26, 115, 232, 0.3);
        border-radius: 6px;
        color: rgb(26, 115, 232);
        font-size: 0.7em;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.2s ease;
        box-shadow: 0 1px 3px rgba(0,0,0,0.1);
        z-index: 1;
      ">↻</button>
      
      <div style="padding: 20px 16px 16px;">
        <div style="
          font-size: 1em;
          line-height: 1.6;
          color: rgb(32, 33, 36);
          margin-bottom: ${actions.length > 0 || dates.length > 0 ? '12px' : '0'};
          font-weight: 400;
        ">${escapeHtml(summaryText)}</div>
        
        ${actions.length > 0 ? `
          <div style="
            margin-top: 12px;
            padding-top: 12px;
            border-top: 1px solid rgba(0,0,0,0.08);
          ">
            <div style="
              font-size: 0.85em;
              font-weight: 600;
              color: rgb(95, 99, 104);
              margin-bottom: 6px;
              letter-spacing: 0.3px;
            ">ACTION ITEMS</div>
            <div style="
              font-size: 0.925em;
              color: rgb(95, 99, 104);
              line-height: 1.5;
            ">
              ${actions.map(a => escapeHtml(a)).join(' • ')}
            </div>
          </div>
        ` : ''}
        
        ${dates.length > 0 ? `
          <div style="
            margin-top: 8px;
            font-size: 0.875em;
            color: rgb(128, 134, 139);
            display: flex;
            align-items: center;
            gap: 4px;
          ">
            📅 ${escapeHtml(dates.join(', '))}
          </div>
        ` : ''}
        
        ${summary.time_ms ? `
          <div style="
            margin-top: 8px;
            font-size: 0.75em;
            color: rgb(154, 160, 166);
            display: flex;
            align-items: center;
            gap: 4px;
            opacity: 0.8;
          ">
            ⏱️ ${formatTime(summary.time_ms)}${summary.cached ? ' (cached)' : ''}
          </div>
        ` : ''}
      </div>
    </div>
    <style>
      @keyframes slideInUp {
        from {
          opacity: 0;
          transform: translateY(20px);
        }
        to {
          opacity: 1;
          transform: translateY(0);
        }
      }
    </style>
  `;

  // inject into Gmail right after sender info (same as loading indicator)
  const emailHeader = document.querySelector('.gE.iv.gt') || 
                       document.querySelector('.gE.iv') ||
                       document.querySelector('[data-message-id]');
  
  if (emailHeader && emailHeader.parentNode) {
    emailHeader.parentNode.insertBefore(summaryDiv, emailHeader.nextSibling);
    console.log('MeTLDR: Summary injected');
    
    // attach event listener to regenerate button (CSP-safe)
    const regenerateBtn = summaryDiv.querySelector('.metldr-regenerate-btn');
    if (regenerateBtn) {
      regenerateBtn.addEventListener('click', async () => {
        const threadId = regenerateBtn.getAttribute('data-thread-id');
        console.log('MeTLDR: Regenerate button clicked for:', threadId);
        await regenerateSummary(threadId);
      });
      
      // add hover effects
      regenerateBtn.addEventListener('mouseenter', () => {
        regenerateBtn.style.background = 'rgba(26, 115, 232, 0.95)';
        regenerateBtn.style.color = 'white';
      });
      regenerateBtn.addEventListener('mouseleave', () => {
        regenerateBtn.style.background = 'rgba(255, 255, 255, 0.95)';
        regenerateBtn.style.color = 'rgb(26, 115, 232)';
      });
    }
  } else {
    console.error('MeTLDR: Could not find email header for summary');
  }
}

// find insertion point in Gmail DOM
function findInsertionPoint() {
  console.log('MeTLDR: Finding insertion point...');
  
  // try Gmail-specific selectors to find where to inject
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
