// background service worker

chrome.sidePanel
  .setPanelBehavior({ openPanelOnActionClick: true })
  .catch((error) => console.error('side panel error:', error));

// ollama wrapper for word lookups
async function callOllama(model, prompt) {
  try {
    const response = await fetch('http://127.0.0.1:11434/api/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model,
        messages: [{ role: 'user', content: prompt }],
        stream: false,
        options: { temperature: 0 }
      })
    });

    if (!response.ok) {
      return { success: false, error: `ollama returned ${response.status}` };
    }

    const data = await response.json();
    const content = data?.message?.content || data?.response;
    
    return { success: true, response: content };
  } catch (error) {
    return { success: false, error: error.message };
  }
}

// message handler for all requests
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SUMMARIZE_EMAIL') {
    handleEmailSummary(message.emailContent, message.emailId, message.metadata, sendResponse, message.forceRegenerate);
    return true;
  }
  
  if (message.type === 'PRE_SUMMARISE') {
    handlePreSummarise(message, sender, sendResponse);
    return true;
  }
  
  if (message.type === 'WORD_LOOKUP') {
    handleWordLookup(message, sendResponse);
    return true;
  }
  
  if (message.action === 'downloadComplete') {
    // clear download flag when dictionary download finishes
    if (message.language) {
      downloadingLanguages.delete(message.language);
    }
    return false;
  }
  
  return false;
});

// indexeddb cache for email summaries
let db = null;

async function initDB() {
  if (db) return db;
  
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('metldr_summaries', 2);
    
    request.onerror = () => {
      console.error('metldr: failed to open indexeddb:', request.error);
      reject(request.error);
    };
    
    request.onsuccess = () => {
      db = request.result;
      resolve(db);
    };
    
    request.onupgradeneeded = (event) => {
      const database = event.target.result;
      
      // create or recreate the summaries store
      if (!database.objectStoreNames.contains('summaries')) {
        database.createObjectStore('summaries', { keyPath: 'emailId' });
      }
    };
  });
}

async function getCachedSummary(emailId) {
  try {
    await initDB();
    // check if object store exists before trying to access it
    if (!db.objectStoreNames.contains('summaries')) {
      return null;
    }
    const transaction = db.transaction(['summaries'], 'readonly');
    const store = transaction.objectStore('summaries');
    const request = store.get(emailId);
    
    return new Promise((resolve) => {
      request.onsuccess = () => resolve(request.result?.summary);
      request.onerror = () => resolve(null);
    });
  } catch (error) {
    console.error('metldr: cache read error:', error);
    return null;
  }
}

async function cacheSummary(emailId, summary) {
  try {
    await initDB();
    // check if object store exists before trying to access it
    if (!db.objectStoreNames.contains('summaries')) {
      return;
    }
    const transaction = db.transaction(['summaries'], 'readwrite');
    const store = transaction.objectStore('summaries');
    await store.put({ emailId, summary, timestamp: Date.now() });
  } catch (error) {
    console.error('metldr: cache write error:', error);
  }
}

async function handleEmailSummary(emailContent, emailId, metadata, sendResponse, forceRegenerate = false) {
  const startTime = Date.now(); // track timing
  
  try {
    // check cache first for instant return (unless regenerating)
    if (!forceRegenerate && emailId) {
      const cached = await getCachedSummary(emailId);
      if (cached) {
        const timeTaken = Date.now() - startTime;
        sendResponse({ 
          summary: { 
            ...cached, 
            time_ms: timeTaken,
            cached: true 
          } 
        });
        return;
      }
    }
    
    // load selected model from chrome.storage
    let selectedModel = 'gemma3:1b'; // default
    try {
      const result = await chrome.storage.local.get('selectedModel');
      if (result.selectedModel) {
        selectedModel = result.selectedModel;
      }
    } catch (error) {
      console.error('metldr: failed to load model selection:', error);
    }
    
    // check if ollama is running (only for new summaries)
    try {
      const healthCheck = await fetch('http://127.0.0.1:11434/api/tags', {
        signal: AbortSignal.timeout(2000)
      });
      if (!healthCheck.ok) {
        throw new Error('ollama not responding');
      }
    } catch (error) {
      console.error('metldr: ollama not available:', error.message);
      sendResponse({
        summary: {
          summary: 'ollama is not running. please start ollama and try again.',
          action_items: [],
          dates: []
        }
      });
      return;
    }
    
    // extract facts first, then summarise from facts
    const extractedFacts = jsExtractFacts(emailContent);
    
    if (!extractedFacts || Object.keys(extractedFacts).length === 0) {
      sendResponse({ 
        summary: {
          summary: 'could not extract facts from email. email may be too long or format is unsupported.',
          action_items: [],
          dates: [],
          confidence: 'low'
        }
      });
      return;
    }
    
    // pass both facts and email body (trim if long)
    const emailSnippet = emailContent.length > 6000 
      ? emailContent.substring(0, 4000) + '\n...[content truncated]...\n' + emailContent.substring(emailContent.length - 2000)
      : emailContent;
    const summary = await generateSummaryFromFacts(extractedFacts, emailSnippet, metadata, selectedModel);
    
    // add timing info and model name
    const timeTaken = Date.now() - startTime;
    summary.time_ms = timeTaken;
    summary.cached = false;
    summary.model = selectedModel;
    
    // cache the summary with emailId in indexeddb
    if (emailId) {
      await cacheSummary(emailId, summary);
    }
    
    sendResponse({ summary });
    
  } catch (error) {
    console.error('metldr background error:', error);
    sendResponse({ 
      summary: {
        summary: `error generating summary: ${error.message}`,
        action_items: [],
        dates: [],
        confidence: 'low'
      }
    });
  }
}

// 1: extract facts from full email
async function extractFactsFromEmail(emailContent) {
  const factsSchema = {
    "type": "object",
    "properties": {
      "amounts": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "label": { "type": "string" },
            "value": { "type": ["number", "string"] },
            "currency": { "type": ["string", "null"] }
          }
        }
      },
      "ids": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "label": { "type": "string" },
            "value": { "type": "string" }
          }
        }
      },
      "dates": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "label": { "type": "string" },
            "when": { "type": "string" }
          }
        }
      },
      "contacts": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "type": { "type": "string" },
            "value": { "type": "string" }
          }
        }
      },
      "links": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "label": { "type": "string" },
            "url": { "type": ["string", "null"] }
          }
        }
      },
      "people": {
        "type": "array",
        "items": { "type": "string" }
      },
      "locations": {
        "type": "array",
        "items": { "type": "string" }
      },
      "action_items": {
        "type": "array",
        "items": { "type": "string" }
      }
    }
  };
  
  // for long emails, use map reduce
  if (emailContent.length > 30000) {
    return await extractFactsMapReduce(emailContent, factsSchema);
  }
  
  // for long emails (> 12000), split into chunks but keep important sections together
  if (emailContent.length > 12000) {
    // prioritise first 8000 chars + last 4000 chars
    const keyContent = emailContent.substring(0, 8000) + '\n\n[END OF EMAIL]\n\n' + emailContent.substring(emailContent.length - 4000);
    return await extractFactsSinglePass(keyContent, factsSchema);
  }
  
  // standard extraction for normal length emails
  return await extractFactsSinglePass(emailContent, factsSchema);
}

async function extractFactsSinglePass(emailContent, schema) {
  const prompt = `extract factual information from this email:

${emailContent}

extract: amounts with currency, reference/tracking ids, dates/times, contact info, action items, and locations mentioned.
only extract information explicitly stated - do not infer or guess.`;

  try {
    const response = await fetch('http://127.0.0.1:11434/api/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model: 'llama3.2:3b',
        messages: [
          { role: 'system', content: 'extract only factual data from emails. return valid json.' },
          { role: 'user', content: prompt }
        ],
        stream: false,
        format: schema,
        options: { temperature: 0 }
      })
    });

    if (!response.ok) {
      throw new Error(`ollama error: ${response.status}`);
    }

    const data = await response.json();
    const content = data?.message?.content || data?.response;
    
    return content ? JSON.parse(content) : null;
  } catch (error) {
    console.error('metldr: fact extraction failed:', error);
    return null;
  }
}

async function extractFactsMapReduce(emailContent, schema) {
  // split email into 15000 char chunks with 2000 char overlap
  const chunkSize = 15000;
  const overlap = 2000;
  const chunks = [];
  
  for (let i = 0; i < emailContent.length; i += chunkSize - overlap) {
    chunks.push(emailContent.substring(i, i + chunkSize));
  }
  
  const allFacts = {
    amounts: [],
    ids: [],
    dates: [],
    contacts: [],
    links: [],
    people: [],
    locations: [],
    action_items: []
  };
  
  // extract facts from each chunk
  for (const chunk of chunks) {
    const facts = await extractFactsSinglePass(chunk, schema);
    if (facts) {
      // merge facts, deduplicating by value
      for (const key in allFacts) {
        if (Array.isArray(allFacts[key]) && Array.isArray(facts[key])) {
          const existing = new Set(JSON.stringify(allFacts[key]));
          for (const item of facts[key]) {
            const itemStr = JSON.stringify(item);
            if (!existing.has(itemStr)) {
              allFacts[key].push(item);
              existing.add(itemStr);
            }
          }
        }
      }
    }
  }
  
  return allFacts;
}

// pass 2: generate summary from extracted facts + email context
async function generateSummaryFromFacts(facts, emailSnippet = '', metadata = null, selectedModel = 'llama3.2:1b') {
  // clean schema for human-readable summaries
  const summarySchema = {
    "type": "object",
    "required": ["summary", "action_items"],
    "properties": {
      "summary": {
        "type": "string",
        "description": "direct 1-2 sentence summary. lead with main action/event. include key details (amounts, dates, ids). no meta phrases."
      },
      "action_items": {
        "type": "array",
        "items": { "type": "string" },
        "description": "concise actions for recipient (e.g., 'check in 3 hours early', 'download app'). max 3-4 items, prioritise most important."
      },
      "key_details": {
        "type": "object",
        "description": "important information extracted from email",
        "properties": {
          "booking_reference": {
            "type": "string",
            "description": "main booking/order/reference id if present"
          },
          "amount": {
            "type": "string",
            "description": "total amount with currency if present"
          },
          "main_date": {
            "type": "string",
            "description": "most important date (departure, deadline, event date)"
          }
        }
      }
    }
  };
  
  // build concise facts string
  const factsText = buildFactsSummary(facts);

  // build metadata context if available
  let metadataContext = '';
  if (metadata) {
    metadataContext = 'email metadata:\n';
    if (metadata.sender) metadataContext += `from: ${metadata.sender}`;
    if (metadata.senderEmail) metadataContext += ` <${metadata.senderEmail}>`;
    if (metadata.sender || metadata.senderEmail) metadataContext += '\n';
    if (metadata.date) metadataContext += `sent: ${metadata.date}\n`;
    if (metadata.subject) metadataContext += `subject: ${metadata.subject}\n`;
    if (metadata.to) metadataContext += `to: ${metadata.to}\n`;
    metadataContext += '\n';
  }

  const systemPrompt = `summarise emails concisely. be direct. skip meta phrases. write for the recipient. use metadata (date, sender) for temporal context when the email mentions vague timings like "tomorrow", "next week", "yesterday".`;

  const userPrompt = `read this email excerpt and the extracted facts, then create a brief summary:

${metadataContext}email excerpt:
${emailSnippet}

extracted facts:
${factsText}

requirements:
- 1-2 sentences maximum
- understand the email's purpose from the excerpt (webinar invite? booking? payment?)
- lead with the main action or event
- include relevant key details from facts (amounts, dates, ids) only if they match the purpose
- skip phrases like "this email", "you received", "the sender"
- use → for routes/paths where relevant
- be direct and scannable`;

  // use selected model with fallback list if it fails
  const models = [selectedModel, 'llama3.2:1b', 'gemma3:4b', 'qwen2.5:3b'].filter((v, i, a) => a.indexOf(v) === i); // remove duplicates
  
  for (const model of models) {
    try {
      // add timeout to prevent hanging
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), 30000); // 30s timeout
      
      const response = await fetch('http://127.0.0.1:11434/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          model,
          messages: [
            { role: 'system', content: systemPrompt },
            { role: 'user', content: userPrompt }
          ],
          stream: false,
          format: summarySchema,
          options: { 
            temperature: 0.3,  // slight randomness for more natural language
            top_p: 0.9
          }
        }),
        signal: controller.signal
      });

      clearTimeout(timeoutId);

      if (!response.ok) {
        continue;
      }

      const data = await response.json();
      const content = data?.message?.content || data?.response;
      if (!content) {
        continue;
      }

      const parsed = JSON.parse(content);
      
      // convert to ui format with intelligent fallbacks
      const mainDate = parsed.key_details?.main_date || (facts.dates?.[0]?.when);
      const bookingRef = parsed.key_details?.booking_reference || (facts.ids?.[0]?.value);
      const amount = parsed.key_details?.amount || (facts.amounts?.[0] ? `${facts.amounts[0].value} ${facts.amounts[0].currency || ''}` : null);
      
      // clean and deduplicate action items (max 3)
      const actionItems = (parsed.action_items || [])
        .filter(item => item && item.length > 3)
        .slice(0, 3);
      
      return {
        summary: parsed.summary || 'no summary generated',
        action_items: actionItems,
        dates: mainDate ? [mainDate] : [],
        key_facts: {
          booking_reference: bookingRef,
          amount: amount
        }
      };
    } catch (err) {
      if (err.name === 'AbortError') {
        continue;
      }
      continue;
    }
  }
  
  // fallback to fact-based summary
  return createSummaryFromFacts(facts);
}

function buildFactsSummary(facts) {
  const lines = [];
  
  if (facts.amounts && facts.amounts.length > 0) {
    lines.push('amounts:');
    facts.amounts.forEach(a => {
      lines.push(`- ${a.label}: ${a.value} ${a.currency || ''}`);
    });
  }
  
  if (facts.ids && facts.ids.length > 0) {
    lines.push('\nids/references:');
    facts.ids.forEach(id => {
      lines.push(`- ${id.label}: ${id.value}`);
    });
  }
  
  if (facts.dates && facts.dates.length > 0) {
    lines.push('\ndates/times:');
    facts.dates.forEach(d => {
      lines.push(`- ${d.label}: ${d.when}`);
    });
  }
  
  if (facts.people && facts.people.length > 0) {
    lines.push('\npeople:');
    facts.people.forEach(p => lines.push(`- ${p}`));
  }
  
  if (facts.locations && facts.locations.length > 0) {
    lines.push('\nlocations:');
    facts.locations.forEach(l => lines.push(`- ${l}`));
  }
  
  if (facts.action_items && facts.action_items.length > 0) {
    lines.push('\naction items:');
    facts.action_items.forEach(a => lines.push(`- ${a}`));
  }
  
  if (facts.contacts && facts.contacts.length > 0) {
    lines.push('\ncontacts:');
    facts.contacts.forEach(c => {
      lines.push(`- ${c.type}: ${c.value}`);
    });
  }
  
  if (facts.links && facts.links.length > 0) {
    lines.push('\nlinks:');
    facts.links.forEach(l => {
      if (l.url) lines.push(`- ${l.label}: ${l.url}`);
    });
  }
  
  return lines.join('\n') || 'no facts extracted';
}

function createSummaryFromFacts(facts) {
  // fallback: create simple summary from facts
  const bullets = [];
  
  if (facts.ids && facts.ids.length > 0) {
    const ids = facts.ids.map(id => `${id.label}: ${id.value}`).join(', ');
    bullets.push(`reference: ${ids}`);
  }
  
  if (facts.amounts && facts.amounts.length > 0) {
    const amounts = facts.amounts.map(a => `${a.value} ${a.currency || ''}`).join(', ');
    bullets.push(`amount: ${amounts}`);
  }
  
  if (facts.dates && facts.dates.length > 0) {
    const dates = facts.dates.map(d => `${d.label}: ${d.when}`).join(', ');
    bullets.push(`dates: ${dates}`);
  }

  // shipping/pickup heuristics (non-domain specific)
  const pickupLoc = (facts.locations && facts.locations[0]) || null;
  const packageId = (facts.ids || []).find(i => /package|parcel|tracking/i.test(i.label));
  if (pickupLoc || packageId) {
    if (packageId && pickupLoc) {
      bullets.unshift(`package ${packageId.value} to ${pickupLoc}`);
    } else if (packageId) {
      bullets.unshift(`package ${packageId.value}`);
    }
  }

  // add deterministic guidance for locker pickups when hints exist
  const hasLocker = /z[- ]?box|locker|pickup point/i.test((facts.locations || []).join(' ') + ' ' + (facts.action_items || []).join(' '));
  if (hasLocker) {
    bullets.push('wait for notification; pick up at locker with code/app');
    bullets.push('enable bluetooth and location; use same phone number in app');
  }
  
  if (bullets.length === 0) {
    bullets.push('email processed but no specific facts extracted');
  }
  
  return {
    summary: bullets.join(' '),
    action_items: facts.action_items || [],
    dates: (facts.dates || []).map(d => typeof d === 'string' ? d : d.when),
    confidence: 'low',
    classification: 'generic',
    key_facts: {
      amounts: facts.amounts || [],
      ids: facts.ids || [],
      contacts: facts.contacts || []
    }
  };
}

// amounts, ids, dates, simple contacts/links
function jsExtractFacts(text) {
  const facts = {
    amounts: [],
    ids: [],
    dates: [],
    contacts: [],
    links: [],
    people: [],
    locations: [],
    action_items: []
  };

  try {
    // amounts
    const amountRegex = /(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?\s?[$€£₹]?[\s]*\d{1,3}(?:[\s.,]\d{3})*(?:[.,]\d{2})?\s?(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?/g;
    const labelHints = [/total/i, /amount/i, /price/i, /fare/i, /paid/i, /due/i];
    const amounts = new Set();
    let m;
    while ((m = amountRegex.exec(text)) !== null) {
      const val = m[0].trim();
      if (val.length < 3) continue;
      const ctxStart = Math.max(0, m.index - 40);
      const ctx = text.substring(ctxStart, m.index + val.length + 10);
      const label = labelHints.find(r => r.test(ctx))?.source.replace(/\//g, '') || 'amount';
      const currency = (val.match(/USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3}/) || [null])[0] || (val.includes('$') ? 'USD' : null);
      const key = `${label}:${val}:${currency || ''}`;
      if (!amounts.has(key)) {
        facts.amounts.push({ label, value: val.replace(/[^0-9.,]/g, ''), currency });
        amounts.add(key);
      }
    }

    // ids (order|booking|invoice|ticket|ref: ABC123-45)
    const idRegex = /(booking|order|invoice|ticket|reference|ref|pnr|record locator)[:#]?\s*([A-Z0-9-]{5,})/gi;
    const ids = new Set();
    while ((m = idRegex.exec(text)) !== null) {
      const label = m[1].toLowerCase().replace(/\s+/g, '_');
      const value = m[2];
      const key = `${label}:${value}`;
      if (!ids.has(key)) {
        facts.ids.push({ label, value });
        ids.add(key);
      }
    }

    // package/parcel/tracking id lines (e.g., "Package number Z 445 5070 212")
    const pkgLineRx = /(package|parcel)\s+number[:#]?\s*([A-Z0-9 \-]{6,})/gi;
    while ((m = pkgLineRx.exec(text)) !== null) {
      const label = 'package_number';
      const value = m[2].replace(/\s{2,}/g, ' ').trim();
      const key = `${label}:${value}`;
      if (!ids.has(key)) { facts.ids.push({ label, value }); ids.add(key); }
    }

    // generic tracking ids (words like tracking/trace code)
    const trackingRx = /(tracking|trace|parcel id|shipment id)[:#]?\s*([A-Z0-9-]{6,})/gi;
    while ((m = trackingRx.exec(text)) !== null) {
      const label = 'tracking';
      const value = m[2];
      const key = `${label}:${value}`;
      if (!ids.has(key)) { facts.ids.push({ label, value }); ids.add(key); }
    }

    // dates (iso-like or human readable common formats)
    const isoDate = /\b\d{4}-\d{2}-\d{2}(?:[T\s]\d{2}:\d{2}(?::\d{2})?)?\b/g;
    const humanDate = /(Mon|Tue|Wed|Thu|Fri|Sat|Sun)[a-z]*,?\s+\d{1,2}\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{4}(?:\s+\d{1,2}:\d{2}(?:\s*[AP]M)?\s*[A-Z]{2,3})?/gi;
    const dateSet = new Set();
    while ((m = isoDate.exec(text)) !== null) {
      const when = m[0];
      if (!dateSet.has(when)) {
        facts.dates.push({ label: 'date', when });
        dateSet.add(when);
      }
    }
    while ((m = humanDate.exec(text)) !== null) {
      const when = m[0];
      if (!dateSet.has(when)) {
        // try infer label by nearby words
        const ctxStart = Math.max(0, m.index - 40);
        const ctx = text.substring(ctxStart, m.index + when.length + 10).toLowerCase();
        const label = /depart|departure|flight|outbound/.test(ctx) ? 'departure'
          : /arriv/.test(ctx) ? 'arrival'
          : /check[- ]?in/.test(ctx) ? 'check-in'
          : 'date';
        facts.dates.push({ label, when });
        dateSet.add(when);
      }
    }

    // contacts (emails/phones simple)
    const emailRx = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
    const phoneRx = /\+?\d[\d\s\-()]{6,}\d/g;
    const seenContact = new Set();
    while ((m = emailRx.exec(text)) !== null) {
      const v = m[0];
      if (!seenContact.has(v)) { facts.contacts.push({ type: 'email', value: v }); seenContact.add(v); }
    }
    while ((m = phoneRx.exec(text)) !== null) {
      const v = m[0].trim();
      if (!seenContact.has(v)) { facts.contacts.push({ type: 'phone', value: v }); seenContact.add(v); }
    }

    // action items (imperatives/common cta verbs)
    const actionLines = text.split(/\n+/).filter(l => /\b(pay|confirm|check[- ]?in|download|track|manage|reset|verify|complete|submit|reply)\b/i.test(l));
    facts.action_items = Array.from(new Set(actionLines.map(l => l.trim()).filter(l => l.length > 0 && l.length < 160))).slice(0, 6);

    // pickup location extraction: lines starting with z-box or similar locker keywords and the next line
    const lines = text.split(/\n+/);
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      if (/^(z[- ]?box|locker|pick[- ]?up point)/i.test(line)) {
        const next = (lines[i + 1] || '').trim();
        const loc = [line.replace(/^z[- ]?box\s*/i, 'z-box'), next].filter(Boolean).join(', ');
        if (loc.length > 8 && !facts.locations.includes(loc)) {
          facts.locations.push(loc);
        }
      }
    }

  } catch (e) {
    console.error('metldr: jsextractfacts failed:', e);
  }

  return facts;
}
// pre-summarisation handler (low priority background processing)
async function handlePreSummarise(message, sender, sendResponse) {
  try {
    const cacheKey = `page:${message.url}`;
    const cached = await getPageSummaryCache(cacheKey);
    
    if (cached && !message.forceRegenerate) {
      sendToSidePanel({ type: 'PAGE_SUMMARY', summary: cached });
      sendResponse({ success: true, cached: true });
      return;
    }
    
    const model = await getBestModel('page_summary');
    
    const prompt = `summarise this ${message.pageType} in exactly 3 clear bullet points. return only valid json:
{"bullets": ["point1", "point2", "point3"], "confidence": 85, "readTime": "5 min"}

content:
${message.content}`;
    
    const result = await callOllama(model, prompt);
    
    if (!result.success) {
      throw new Error(result.error);
    }
    
    const summary = JSON.parse(result.response);
    summary.pageType = message.pageType;
    summary.metadata = message.metadata;
    summary.sections = message.sections || [];
    summary.timestamp = Date.now();
    
    await cachePageSummary(cacheKey, summary, 3600);
    
    sendToSidePanel({ type: 'PAGE_SUMMARY', summary });
    
    sendResponse({ success: true, cached: false });
  } catch (error) {
    console.error('metldr: pre-summarisation failed:', error);
    sendResponse({ success: false, error: error.message });
  }
}

// extract json from llm response (handles markdown, extra text)
function extractJSON(text) {
  try {
    return JSON.parse(text);
  } catch {
    // try to find json object in response
    const jsonMatch = text.match(/\{[^{}]*\}/);
    if (jsonMatch) {
      return JSON.parse(jsonMatch[0]);
    }
    throw new Error('no valid json found in response');
  }
}

// track ongoing downloads to prevent duplicates
const downloadingLanguages = new Set();

// cache db connection for faster lookups
let cachedDictDb = null;
let cachedLanguages = null;
let cacheTimestamp = 0;
const CACHE_TTL = 30000; // 30 seconds

async function getCachedSettings() {
  const now = Date.now();
  if (cachedLanguages && (now - cacheTimestamp) < CACHE_TTL) {
    return cachedLanguages;
  }
  
  const settings = await chrome.storage.local.get(['selectedLanguages']);
  let languages = settings.selectedLanguages || ['en'];
  
  // ensure languages is an array
  if (!Array.isArray(languages)) {
    if (typeof languages === 'object') {
      languages = Object.values(languages);
    } else {
      languages = [languages];
    }
  }
  
  cachedLanguages = languages;
  cacheTimestamp = now;
  return languages;
}

async function getCachedDb() {
  if (cachedDictDb) return cachedDictDb;
  
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('metldr-dictionary', 1);
    request.onsuccess = () => {
      cachedDictDb = request.result;
      resolve(cachedDictDb);
    };
    request.onerror = () => reject(request.error);
  });
}

// word lookup handler with three-tier fallback: api → local → ollama
async function handleWordLookup(message, sendResponse) {
  const startTime = performance.now();
  
  try {
    // get settings
    const settings = await chrome.storage.local.get(['selectedLanguages', 'dictionarySource', 'selectedModel']);
    let languages = settings.selectedLanguages || ['en'];
    const dictionarySource = settings.dictionarySource || 'api';
    const userModel = settings.selectedModel || null;
    
    // ensure languages is an array
    if (!Array.isArray(languages)) {
      if (typeof languages === 'object') {
        languages = Object.values(languages);
      } else {
        languages = [languages];
      }
    }
    
    const word = message.word.toLowerCase().trim();
    
    // step 1: try local db only if user chose 'local' and not downloading
    if (dictionarySource === 'local') {
      let shouldTryLocal = true;
      for (const langCode of languages) {
        if (downloadingLanguages.has(langCode)) {
          shouldTryLocal = false;
          break;
        }
      }
      
      if (shouldTryLocal) {
        try {
          const dictDb = await getCachedDb();
        
          for (const langCode of languages) {
            try {
              const tx = dictDb.transaction([langCode], 'readonly');
              const store = tx.objectStore(langCode);
              const result = await new Promise((resolve, reject) => {
                const request = store.get(word);
                request.onsuccess = () => resolve(request.result);
                request.onerror = () => reject(request.error);
              });
              
              if (result) {
                const elapsed = performance.now() - startTime;
                sendResponse({ 
                  success: true, 
                  result: {
                    definitions: [{
                      definition: result.definition,
                      partOfSpeech: result.pos,
                      example: null
                    }],
                    language: langCode,
                    source: 'local'
                  }
                });
                return;
              }
            } catch (err) {
              continue;
            }
          }
        } catch (dbError) {
          console.error('[word_lookup] local db error, falling back to api:', dbError);
        }
      }
    }
    
    // step 2: fallback to free dictionary api
    let apiFound = false;
    for (const langCode of languages) {
      try {
        const url = `https://api.dictionaryapi.dev/api/v2/entries/${langCode}/${word}`;
        
        const response = await fetch(url);
        
        if (response.ok) {
          const data = await response.json();
          
          if (data && data[0] && data[0].meanings) {
            // collect all definitions from all meanings
            const allDefinitions = [];
            const preferredPos = ['noun', 'verb', 'adjective', 'adverb'];
            
            // prioritise common parts of speech
            const sortedMeanings = [...data[0].meanings].sort((a, b) => {
              const aIndex = preferredPos.indexOf(a.partOfSpeech);
              const bIndex = preferredPos.indexOf(b.partOfSpeech);
              if (aIndex === -1 && bIndex === -1) return 0;
              if (aIndex === -1) return 1;
              if (bIndex === -1) return -1;
              return aIndex - bIndex;
            });
            
            // extract all clean definitions
            for (const meaning of sortedMeanings) {
              if (meaning.definitions && meaning.definitions.length > 0) {
                for (const def of meaning.definitions) {
                  const defText = (def.definition || '').toLowerCase();
                  
                  // filter out vulgar/archaic/obsolete
                  if (defText.includes('vulgar') || 
                      defText.includes('archaic') || 
                      defText.includes('obsolete') || 
                      defText.length < 10) {
                    continue;
                  }
                  
                  allDefinitions.push({
                    definition: def.definition,
                    partOfSpeech: meaning.partOfSpeech || 'unknown',
                    example: def.example || null
                  });
                }
              }
            }
            
            if (allDefinitions.length === 0) {
              // fallback: use first definition even if filtered
              const firstMeaning = data[0].meanings[0];
              allDefinitions.push({
                definition: firstMeaning.definitions[0].definition,
                partOfSpeech: firstMeaning.partOfSpeech || 'unknown',
                example: firstMeaning.definitions[0].example || null
              });
            }
            
            sendResponse({ 
              success: true, 
              result: {
                definitions: allDefinitions,
                language: langCode,
                source: 'api'
              }
            });
            
            apiFound = true;
            
            // trigger background download (if not already downloading)
            if (!downloadingLanguages.has(langCode)) {
              downloadingLanguages.add(langCode);
              
              // notify side panel to start download
              chrome.runtime.sendMessage({
                action: 'startBackgroundDownload',
                language: langCode
              }).catch(() => {});
              
              // cleanup flag after 5 minutes (timeout)
              setTimeout(() => {
                downloadingLanguages.delete(langCode);
              }, 300000);
            }
            
            return;
          }
        }
      } catch (err) {
        continue;
      }
    }
    
    // step 3: if api failed, try local dictionary (regardless of user preference)
    if (!apiFound && dictionarySource === 'api') {
      try {
        const dictDb = await getCachedDb();
      
        for (const langCode of languages) {
          try {
            const tx = dictDb.transaction([langCode], 'readonly');
            const store = tx.objectStore(langCode);
            const result = await new Promise((resolve, reject) => {
              const request = store.get(word);
              request.onsuccess = () => resolve(request.result);
              request.onerror = () => reject(request.error);
            });
            
            if (result) {
              const elapsed = performance.now() - startTime;
              sendResponse({ 
                success: true, 
                result: {
                  definitions: [{
                    definition: result.definition,
                    partOfSpeech: result.pos,
                    example: null
                  }],
                  language: langCode,
                  source: 'local'
                }
              });
              return;
            }
          } catch (err) {
            continue;
          }
        }
      } catch (dbError) {
        console.error('[word_lookup] local fallback failed:', dbError);
      }
    }
    
    // step 4: final fallback to ollama llm
    try {
      // import ollama client dynamically
      const { OllamaClient } = await import(chrome.runtime.getURL('lib/OllamaClient.js'));
      const ollamaClient = new OllamaClient();
      
      // check if ollama is available
      const { connected, models } = await ollamaClient.checkConnection();
      if (!connected || models.length === 0) {
        throw new Error('ollama not available');
      }
      
      // prefer smallest model for quick definitions
      const smallModels = ['llama3.2:1b', 'qwen2.5:1.5b', 'llama3.2:3b'];
      let model = userModel || models[0];
      
      // find smallest available model if no user preference
      if (!userModel) {
        for (const small of smallModels) {
          const found = models.find(m => m.includes(small));
          if (found) {
            model = found;
            break;
          }
        }
      }
      
      // generate one-line definition
      const prompt = `define the word "${word}" in one concise sentence (15-20 words max). include part of speech in parentheses.`;
      const definition = await ollamaClient.generate(prompt, {
        model,
        temperature: 0.3,
        system: 'you are a concise dictionary. respond with only the definition, nothing else.'
      });
      
      const elapsed = performance.now() - startTime;
      
      sendResponse({ 
        success: true, 
        result: {
          definitions: [{
            definition: definition.trim(),
            partOfSpeech: 'generated',
            example: null
          }],
          language: 'en',
          source: 'ollama'
        }
      });
      return;
    } catch (ollamaErr) {
      console.error('[word_lookup] ollama fallback failed:', ollamaErr);
    }
    
    // absolute failure - no definition anywhere
    sendResponse({ 
      success: false, 
      error: 'word not found in any dictionary source or llm unavailable' 
    });
  } catch (error) {
    console.error('metldr: word lookup failed:', error);
    sendResponse({ 
      success: false, 
      error: 'lookup failed: ' + error.message 
    });
  }
}

function sendToSidePanel(message) {
  chrome.runtime.sendMessage(message).catch(err => {});
}

const pageSummaryCache = new Map();

async function getPageSummaryCache(key) {
  return pageSummaryCache.get(key);
}

async function cachePageSummary(key, summary, ttlSeconds) {
  pageSummaryCache.set(key, summary);
  setTimeout(() => {
    pageSummaryCache.delete(key);
  }, ttlSeconds * 1000);
}

async function getBestModel(taskType) {
  const models = await getAvailableModels();
  
  if (taskType === 'word_lookup') {
    return models.find(m => m.includes('llama3.2:1b')) || models[0];
  }
  
  if (taskType === 'page_summary') {
    return models.find(m => m.includes('llama3.2:3b')) || models.find(m => m.includes('llama3.2:1b')) || models[0];
  }
  
  return models[0];
}

async function getAvailableModels() {
  try {
    const response = await fetch('http://127.0.0.1:11434/api/tags');
    if (!response.ok) return [];
    
    const data = await response.json();
    return data.models?.map(m => m.name) || [];
  } catch (e) {
    return [];
  }
}
