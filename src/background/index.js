// Background service worker - Ollama-powered email summarization
// Uses two-pass approach: 1) Extract facts (JS + LLM), 2) Generate summary from facts
// All LLM calls use Ollama with structured JSON output (temperature=0 for consistency)

chrome.sidePanel
  .setPanelBehavior({ openPanelOnActionClick: true })
  .catch((error) => console.error('Side panel error:', error));

// handle email summarization requests from content script
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SUMMARIZE_EMAIL') {
    handleEmailSummary(message.emailContent, message.emailId, sendResponse, message.forceRegenerate);
    return true; // keep channel open for async
  }
  return false; // not handled
});

// IndexedDB cache for email summaries
let db = null;

async function initDB() {
  if (db) return db;
  
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('metldr_summaries', 1);
    
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      db = request.result;
      resolve(db);
    };
    
    request.onupgradeneeded = (event) => {
      const database = event.target.result;
      if (!database.objectStoreNames.contains('summaries')) {
        database.createObjectStore('summaries', { keyPath: 'emailId' });
      }
    };
  });
}

async function getCachedSummary(emailId) {
  try {
    await initDB();
    const transaction = db.transaction(['summaries'], 'readonly');
    const store = transaction.objectStore('summaries');
    const request = store.get(emailId);
    
    return new Promise((resolve) => {
      request.onsuccess = () => resolve(request.result?.summary);
      request.onerror = () => resolve(null);
    });
  } catch (error) {
    console.error('MeTLDR: Cache read error:', error);
    return null;
  }
}

async function cacheSummary(emailId, summary) {
  try {
    await initDB();
    const transaction = db.transaction(['summaries'], 'readwrite');
    const store = transaction.objectStore('summaries');
    await store.put({ emailId, summary, timestamp: Date.now() });
    console.log('MeTLDR: Cached summary for:', emailId);
  } catch (error) {
    console.error('MeTLDR: Cache write error:', error);
  }
}

async function handleEmailSummary(emailContent, emailId, sendResponse, forceRegenerate = false) {
  const startTime = Date.now(); // Track timing
  
  try {
    console.log('MeTLDR background: Processing email summary for:', emailId);
    console.log('MeTLDR: Email content length:', emailContent.length);
    
    // Check if Ollama is running
    try {
      const healthCheck = await fetch('http://127.0.0.1:11434/api/tags', {
        signal: AbortSignal.timeout(2000)
      });
      if (!healthCheck.ok) {
        throw new Error('Ollama not responding');
      }
    } catch (error) {
      console.error('MeTLDR: Ollama not available:', error.message);
      sendResponse({
        summary: {
          summary: 'Ollama is not running. Please start Ollama and try again.',
          action_items: [],
          dates: []
        }
      });
      return;
    }
    
    // check cache first (unless regenerating)
    if (!forceRegenerate && emailId) {
      const cached = await getCachedSummary(emailId);
      if (cached) {
        const timeTaken = Date.now() - startTime;
        console.log('MeTLDR: Returning cached summary for:', emailId);
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
    
    // two-pass approach: extract facts first, then summarize from facts
    // this reduces hallucination and handles long emails better
    console.log('MeTLDR: Pass 1 - Extracting facts from full email (JS fast path)...');
    const extractedFacts = jsExtractFacts(emailContent);
    console.log('MeTLDR: Extracted facts:', extractedFacts);
    
    if (!extractedFacts || Object.keys(extractedFacts).length === 0) {
      console.warn('MeTLDR: No facts extracted, using fallback');
      sendResponse({ 
        summary: {
          summary: 'Could not extract facts from email. Email may be too long or format is unsupported.',
          action_items: [],
          dates: [],
          confidence: 'low'
        }
      });
      return;
    }
    
    console.log('MeTLDR: Pass 2 - Generating summary from facts + context...');
    // Pass both facts and email body (trim if extremely long)
    const emailSnippet = emailContent.length > 6000 
      ? emailContent.substring(0, 4000) + '\n...[content truncated]...\n' + emailContent.substring(emailContent.length - 2000)
      : emailContent;
    const summary = await generateSummaryFromFacts(extractedFacts, emailSnippet);
    
    // Add timing info
    const timeTaken = Date.now() - startTime;
    summary.time_ms = timeTaken;
    summary.cached = false;
    
    // cache the summary with emailId in IndexedDB
    if (emailId) {
      await cacheSummary(emailId, summary);
    }
    
    sendResponse({ summary });
    
  } catch (error) {
    console.error('MeTLDR background error:', error);
    sendResponse({ 
      summary: {
        summary: `Error generating summary: ${error.message}`,
        action_items: [],
        dates: [],
        confidence: 'low'
      }
    });
  }
}

// Pass 1: Extract facts from full email (smart chunking for long emails)
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
  
  // for very long emails (> 30000 chars), use map-reduce
  if (emailContent.length > 30000) {
    console.log('MeTLDR: Email very long, using map-reduce extraction');
    return await extractFactsMapReduce(emailContent, factsSchema);
  }
  
  // for long emails (> 12000), split into chunks but keep important sections together
  if (emailContent.length > 12000) {
    console.log('MeTLDR: Email long, extracting from key sections');
    // prioritize: first 8000 chars + last 4000 chars (often has important details)
    const keyContent = emailContent.substring(0, 8000) + '\n\n[END OF EMAIL]\n\n' + emailContent.substring(emailContent.length - 4000);
    return await extractFactsSinglePass(keyContent, factsSchema);
  }
  
  // standard extraction for normal length emails
  return await extractFactsSinglePass(emailContent, factsSchema);
}

async function extractFactsSinglePass(emailContent, schema) {
  const prompt = `Extract factual information from this email:

${emailContent}

Extract: amounts with currency, reference/tracking IDs, dates/times, contact info, action items, and locations mentioned.
Only extract information explicitly stated - do not infer or guess.`;

  try {
    const response = await fetch('http://127.0.0.1:11434/api/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model: 'llama3.2:3b',
        messages: [
          { role: 'system', content: 'Extract only factual data from emails. Return valid JSON.' },
          { role: 'user', content: prompt }
        ],
        stream: false,
        format: schema,
        options: { temperature: 0 }
      })
    });

    if (!response.ok) {
      throw new Error(`Ollama error: ${response.status}`);
    }

    const data = await response.json();
    const content = data?.message?.content || data?.response;
    
    return content ? JSON.parse(content) : null;
  } catch (error) {
    console.error('MeTLDR: Fact extraction failed:', error);
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
  
  console.log(`MeTLDR: Split into ${chunks.length} chunks for map-reduce`);
  
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

// Pass 2: Generate summary from extracted facts + email context
async function generateSummaryFromFacts(facts, emailSnippet = '') {
  // Clean schema for human-readable summaries
  const summarySchema = {
    "type": "object",
    "required": ["summary", "action_items"],
    "properties": {
      "summary": {
        "type": "string",
        "description": "Direct 1-2 sentence summary. Lead with main action/event. Include key details (amounts, dates, IDs). No meta phrases."
      },
      "action_items": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Concise actions for recipient (e.g., 'Check in 3 hours early', 'Download app'). Max 3-4 items, prioritize most important."
      },
      "key_details": {
        "type": "object",
        "description": "Important information extracted from email",
        "properties": {
          "booking_reference": {
            "type": "string",
            "description": "Main booking/order/reference ID if present"
          },
          "amount": {
            "type": "string",
            "description": "Total amount with currency if present"
          },
          "main_date": {
            "type": "string",
            "description": "Most important date (departure, deadline, event date)"
          }
        }
      }
    }
  };
  
  // Build concise facts string
  const factsText = buildFactsSummary(facts);

  const systemPrompt = `Summarize emails concisely. Be direct. Skip meta phrases. Write for the recipient.`;

  const userPrompt = `Read this email excerpt and the extracted facts, then create a brief summary:

EMAIL EXCERPT:
${emailSnippet}

EXTRACTED FACTS:
${factsText}

Requirements:
- 1-2 sentences maximum
- Understand the email's PURPOSE from the excerpt (webinar invite? booking? payment?)
- Lead with the main action or event
- Include relevant key details from facts (amounts, dates, IDs) ONLY if they match the purpose
- Skip phrases like "this email", "you received", "the sender"
- Use → for routes/paths where relevant
- Be direct and scannable`;

  // Try models with fallback (30s timeout each)
  const models = ['gemma3:4b', 'llama3.2:1b', 'qwen2.5:3b'];
  
  for (const model of models) {
    try {
      console.log(`MeTLDR: Trying model ${model}...`);
      
      // Add timeout to prevent hanging
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
            temperature: 0.3,  // Slight randomness for more natural language
            top_p: 0.9
          }
        }),
        signal: controller.signal
      });

      clearTimeout(timeoutId);

      if (!response.ok) {
        console.warn(`MeTLDR: Model ${model} returned ${response.status}`);
        continue;
      }

      const data = await response.json();
      const content = data?.message?.content || data?.response;
      if (!content) {
        console.warn(`MeTLDR: Model ${model} returned empty content`);
        continue;
      }

      const parsed = JSON.parse(content);
      console.log(`MeTLDR: Successfully generated summary with ${model}`);
      console.log('MeTLDR: Summary:', parsed.summary);
      
      // Convert to UI format with intelligent fallbacks
      const mainDate = parsed.key_details?.main_date || (facts.dates?.[0]?.when);
      const bookingRef = parsed.key_details?.booking_reference || (facts.ids?.[0]?.value);
      const amount = parsed.key_details?.amount || (facts.amounts?.[0] ? `${facts.amounts[0].value} ${facts.amounts[0].currency || ''}` : null);
      
      // Clean and deduplicate action items (max 3)
      const actionItems = (parsed.action_items || [])
        .filter(item => item && item.length > 3)
        .slice(0, 3);
      
      return {
        summary: parsed.summary || 'No summary generated',
        action_items: actionItems,
        dates: mainDate ? [mainDate] : [],
        key_facts: {
          booking_reference: bookingRef,
          amount: amount
        }
      };
    } catch (err) {
      if (err.name === 'AbortError') {
        console.warn(`MeTLDR: Model ${model} timed out after 30s`);
      } else {
        console.warn(`MeTLDR: Model ${model} failed:`, err.message);
      }
      continue;
    }
  }
  
  // Fallback to fact-based summary
  console.warn('MeTLDR: All models failed, using fallback');
  return createSummaryFromFacts(facts);
}

function buildFactsSummary(facts) {
  const lines = [];
  
  if (facts.amounts && facts.amounts.length > 0) {
    lines.push('AMOUNTS:');
    facts.amounts.forEach(a => {
      lines.push(`- ${a.label}: ${a.value} ${a.currency || ''}`);
    });
  }
  
  if (facts.ids && facts.ids.length > 0) {
    lines.push('\nIDs/REFERENCES:');
    facts.ids.forEach(id => {
      lines.push(`- ${id.label}: ${id.value}`);
    });
  }
  
  if (facts.dates && facts.dates.length > 0) {
    lines.push('\nDATES/TIMES:');
    facts.dates.forEach(d => {
      lines.push(`- ${d.label}: ${d.when}`);
    });
  }
  
  if (facts.people && facts.people.length > 0) {
    lines.push('\nPEOPLE:');
    facts.people.forEach(p => lines.push(`- ${p}`));
  }
  
  if (facts.locations && facts.locations.length > 0) {
    lines.push('\nLOCATIONS:');
    facts.locations.forEach(l => lines.push(`- ${l}`));
  }
  
  if (facts.action_items && facts.action_items.length > 0) {
    lines.push('\nACTION ITEMS:');
    facts.action_items.forEach(a => lines.push(`- ${a}`));
  }
  
  if (facts.contacts && facts.contacts.length > 0) {
    lines.push('\nCONTACTS:');
    facts.contacts.forEach(c => {
      lines.push(`- ${c.type}: ${c.value}`);
    });
  }
  
  if (facts.links && facts.links.length > 0) {
    lines.push('\nLINKS:');
    facts.links.forEach(l => {
      if (l.url) lines.push(`- ${l.label}: ${l.url}`);
    });
  }
  
  return lines.join('\n') || 'No facts extracted';
}

function createSummaryFromFacts(facts) {
  // fallback: create simple summary from facts
  const bullets = [];
  
  if (facts.ids && facts.ids.length > 0) {
    const ids = facts.ids.map(id => `${id.label}: ${id.value}`).join(', ');
    bullets.push(`Reference: ${ids}`);
  }
  
  if (facts.amounts && facts.amounts.length > 0) {
    const amounts = facts.amounts.map(a => `${a.value} ${a.currency || ''}`).join(', ');
    bullets.push(`Amount: ${amounts}`);
  }
  
  if (facts.dates && facts.dates.length > 0) {
    const dates = facts.dates.map(d => `${d.label}: ${d.when}`).join(', ');
    bullets.push(`Dates: ${dates}`);
  }

  // shipping/pickup heuristics (non-domain specific)
  const pickupLoc = (facts.locations && facts.locations[0]) || null;
  const packageId = (facts.ids || []).find(i => /package|parcel|tracking/i.test(i.label));
  if (pickupLoc || packageId) {
    if (packageId && pickupLoc) {
      bullets.unshift(`Package ${packageId.value} to ${pickupLoc}`);
    } else if (packageId) {
      bullets.unshift(`Package ${packageId.value}`);
    }
  }

  // add deterministic guidance for locker pickups when hints exist
  const hasLocker = /z[- ]?box|locker|pickup point/i.test((facts.locations || []).join(' ') + ' ' + (facts.action_items || []).join(' '));
  if (hasLocker) {
    bullets.push('Wait for notification; pick up at locker with code/app');
    bullets.push('Enable Bluetooth and location; use same phone number in app');
  }
  
  if (bullets.length === 0) {
    bullets.push('Email processed but no specific facts extracted');
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

// fast JS extractor: amounts, ids, dates, simple contacts/links
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

    // dates (ISO-like or human readable common formats)
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

    // action items (imperatives/common CTA verbs)
    const actionLines = text.split(/\n+/).filter(l => /\b(pay|confirm|check[- ]?in|download|track|manage|reset|verify|complete|submit|reply)\b/i.test(l));
    facts.action_items = Array.from(new Set(actionLines.map(l => l.trim()).filter(l => l.length > 0 && l.length < 160))).slice(0, 6);

    // pickup location extraction: lines starting with Z-BOX or similar locker keywords and the next line
    const lines = text.split(/\n+/);
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i].trim();
      if (/^(z[- ]?box|locker|pick[- ]?up point)/i.test(line)) {
        const next = (lines[i + 1] || '').trim();
        const loc = [line.replace(/^z[- ]?box\s*/i, 'Z-BOX'), next].filter(Boolean).join(', ');
        if (loc.length > 8 && !facts.locations.includes(loc)) {
          facts.locations.push(loc);
        }
      }
    }

  } catch (e) {
    console.error('MeTLDR: jsExtractFacts failed:', e);
  }

  return facts;
}