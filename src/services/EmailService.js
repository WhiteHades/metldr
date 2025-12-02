import { OllamaService } from './OllamaService.js';
import { cacheService } from './CacheService.js';

export class EmailService {
  static async summarize(emailContent, emailId = null, metadata = null, force = false) {
    const startTime = Date.now();

    try {
      if (!force && emailId) {
        const cached = await cacheService.getEmailSummary(emailId);
        if (cached) {
          const elapsed = Date.now() - startTime;          
          this._maybeGenerateReplies(emailId, emailContent, cached, metadata);
          return { ...cached, time_ms: elapsed, cached: true };
        }
      }

      const { available } = await OllamaService.checkAvailable();
      if (!available) throw new Error('ollama not available');

      const facts = this._extractFacts(emailContent);
      if (!facts || !Object.keys(facts).length) {
        throw new Error('no facts extracted');
      }

      const model = await OllamaService.selectBest('email_summary');
      if (!model) throw new Error('no models available');

      const snippet = emailContent.length > 6000
        ? emailContent.substring(0, 4000) + '\n...[truncated]...\n' + emailContent.substring(emailContent.length - 2000)
        : emailContent;

      const summary = await this._generateSummary(facts, snippet, metadata, model);

      const elapsed = Date.now() - startTime;
      summary.time_ms = elapsed;
      summary.cached = false;
      summary.model = model;

      if (emailId) {
        await cacheService.setEmailSummary(emailId, summary);
        this._generateRepliesBackground(emailId, emailContent, summary, metadata);
      }

      return summary;
    } catch (err) {
      console.error('[EmailService.summarize]', err.message);
      throw err;
    }
  }

  static async _maybeGenerateReplies(emailId, emailContent, summary, metadata) {
    try {
      const cached = await cacheService.getReplySuggestions(emailId);
      if (!cached) {
        this._generateRepliesBackground(emailId, emailContent, summary, metadata);
      }
    } catch (err) {
      console.error('[EmailService._maybeGenerateReplies]', err.message);
    }
  }

  static _generateRepliesBackground(emailId, emailContent, summary, metadata) {
    this.generateReplySuggestions(emailId, emailContent, summary, metadata)
      .then(suggestions => {
        if (suggestions?.length > 0) {
          console.log('[EmailService] generated', suggestions.length, 'reply suggestions for', emailId);
        }
      })
      .catch(err => {
        console.error('[EmailService._generateRepliesBackground]', err.message);
      });
  }

  static async generateReplySuggestions(emailId, emailContent, summary, metadata = null) {
    try {
      const cached = await cacheService.getReplySuggestions(emailId);
      if (cached) return cached;

      const { available } = await OllamaService.checkAvailable();
      if (!available) return [];

      const model = await OllamaService.selectBest('email_summary');
      if (!model) return [];

      const snippet = emailContent.length > 4000
        ? emailContent.substring(0, 3000) + '\n...[truncated]...\n' + emailContent.substring(emailContent.length - 1000)
        : emailContent;

      const suggestions = await this._generateReplies(snippet, summary, metadata, model);
      
      if (suggestions?.length > 0 && emailId) {
        await cacheService.setReplySuggestions(emailId, suggestions);
      }

      return suggestions;
    } catch (err) {
      console.error('[EmailService.generateReplySuggestions]', err.message);
      return [];
    }
  }

  static async _generateReplies(snippet, summary, metadata, model) {
    const schema = {
      type: 'object',
      required: ['replies'],
      properties: {
        replies: {
          type: 'array',
          items: {
            type: 'object',
            required: ['tone', 'length', 'body'],
            properties: {
              tone: {
                type: 'string',
                enum: ['professional', 'friendly', 'brief', 'detailed'],
                description: 'The tone/style of this reply'
              },
              length: {
                type: 'string',
                enum: ['short', 'medium', 'long'],
                description: 'short=1-2 sentences, medium=1 paragraph, long=2+ paragraphs'
              },
              body: {
                type: 'string',
                description: 'The actual reply text. Do NOT include subject line, greeting/sign-off can be minimal or omitted for short replies.'
              },
              label: {
                type: 'string',
                description: 'A 2-4 word label describing this reply option (e.g., "Quick Confirm", "Detailed Response", "Polite Decline")'
              }
            }
          },
          minItems: 3,
          maxItems: 5,
          description: 'Array of suggested reply options with varying tones and lengths'
        }
      }
    };

    let metadataCtx = '';
    if (metadata) {
      metadataCtx = 'ORIGINAL EMAIL INFO:\n';
      if (metadata.sender) metadataCtx += `From: ${metadata.sender}`;
      if (metadata.senderEmail) metadataCtx += ` <${metadata.senderEmail}>`;
      if (metadata.sender || metadata.senderEmail) metadataCtx += '\n';
      if (metadata.subject) metadataCtx += `Subject: ${metadata.subject}\n`;
      metadataCtx += '\n';
    }

    const summaryCtx = summary ? `
SUMMARY OF EMAIL:
- Intent: ${summary.intent || 'unknown'}
- Summary: ${summary.summary || ''}
- Action Items: ${(summary.action_items || []).join(', ') || 'none'}
- Urgency: ${summary.urgency || 'normal'}
` : '';

    const systemPrompt = `You are an expert email assistant. Generate diverse reply options that the user can quickly select and send.

RULES:
1. Generate 3-5 distinct reply options with different tones/lengths
2. Always include: one brief/professional, one friendly, one detailed (if appropriate)
3. Match the formality level of the original email
4. Keep "short" replies to 1-2 sentences max
5. For "medium" replies, use 1 short paragraph
6. For "long" replies, be thorough but concise (2-3 paragraphs max)
7. Do NOT include email headers/subject lines
8. Greetings/sign-offs should be minimal or omitted for short replies
9. Make replies context-aware based on email content and action items
10. For confirmation-type emails, include a quick "Got it" option
11. For questions, provide helpful and direct answers
12. Each reply should be immediately sendable with minimal editing

OUTPUT: JSON only. No explanations.`;

    const userPrompt = `Generate reply suggestions for this email.

${metadataCtx}${summaryCtx}
---
ORIGINAL EMAIL:
${snippet}
---

Generate 3-5 reply options with varying tones (professional/friendly/brief/detailed) and lengths (short/medium/long).`;

    try {
      const result = await OllamaService.complete(
        model,
        [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: userPrompt }
        ],
        { format: schema, temperature: 0.4, top_p: 0.9 }
      );

      if (!result.ok) {
        console.error('[EmailService._generateReplies]', result.error);
        return [];
      }

      const parsed = JSON.parse(result.content);
      return (parsed.replies || []).map((r, idx) => ({
        id: `reply_${idx}`,
        tone: r.tone || 'professional',
        length: r.length || 'medium',
        body: r.body || '',
        label: r.label || `Option ${idx + 1}`
      }));
    } catch (err) {
      console.error('[EmailService._generateReplies] parse error:', err.message);
      return [];
    }
  }

  static async getCachedReplies(emailId) {
    try {
      return await cacheService.getReplySuggestions(emailId);
    } catch (err) {
      console.error('[EmailService.getCachedReplies]', err.message);
      return null;
    }
  }

  static _extractFacts(text) {
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
      const amountRx = /(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?\s?[$€£₹]?[\s]*\d{1,3}(?:[\s.,]\d{3})*(?:[.,]\d{2})?\s?(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?/g;
      const amountLabels = [/total/i, /amount/i, /price/i, /fare/i, /paid/i, /due/i];
      const seenAmounts = new Set();
      let m;

      while ((m = amountRx.exec(text)) !== null) {
        const val = m[0].trim();
        if (val.length < 3) continue;

        const ctx = text.substring(Math.max(0, m.index - 40), m.index + val.length + 10);
        const label = amountLabels.find(r => r.test(ctx))?.source.replace(/\//g, '') || 'amount';
        const curr = (val.match(/USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3}/) || [null])[0] || (val.includes('$') ? 'USD' : null);
        const key = `${label}:${val}:${curr || ''}`;

        if (!seenAmounts.has(key)) {
          facts.amounts.push({ label, value: val.replace(/[^0-9.,]/g, ''), currency: curr });
          seenAmounts.add(key);
        }
      }

      const idPatterns = [
        /(booking|order|invoice|ticket|reference|ref|pnr|record locator)[:#]?\s*([A-Z0-9-]{5,})/gi,
        /(package|parcel)\s+number[:#]?\s*([A-Z0-9 \-]{6,})/gi,
        /(tracking|trace|parcel id|shipment id)[:#]?\s*([A-Z0-9-]{6,})/gi
      ];
      const seenIds = new Set();

      for (const pattern of idPatterns) {
        while ((m = pattern.exec(text)) !== null) {
          const label = m[1].toLowerCase().replace(/\s+/g, '_');
          const value = m[2].replace(/\s{2,}/g, ' ').trim();
          const key = `${label}:${value}`;

          if (!seenIds.has(key)) {
            facts.ids.push({ label, value });
            seenIds.add(key);
          }
        }
      }

      const isoDateRx = /\b\d{4}-\d{2}-\d{2}(?:[T\s]\d{2}:\d{2}(?::\d{2})?)?\b/g;
      const humanDateRx = /(Mon|Tue|Wed|Thu|Fri|Sat|Sun)[a-z]*,?\s+\d{1,2}\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{4}(?:\s+\d{1,2}:\d{2}(?:\s*[AP]M)?\s*[A-Z]{2,3})?/gi;
      const seenDates = new Set();

      while ((m = isoDateRx.exec(text)) !== null) {
        const when = m[0];
        if (!seenDates.has(when)) {
          facts.dates.push({ label: 'date', when });
          seenDates.add(when);
        }
      }

      while ((m = humanDateRx.exec(text)) !== null) {
        const when = m[0];
        if (!seenDates.has(when)) {
          const ctx = text.substring(Math.max(0, m.index - 40), m.index + when.length + 10).toLowerCase();
          const label = /depart|departure|flight|outbound/.test(ctx) ? 'departure'
            : /arriv/.test(ctx) ? 'arrival'
            : /check[- ]?in/.test(ctx) ? 'check-in'
            : 'date';

          facts.dates.push({ label, when });
          seenDates.add(when);
        }
      }

      const emailRx = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
      const phoneRx = /\+?\d[\d\s\-()]{6,}\d/g;
      const seenContact = new Set();

      while ((m = emailRx.exec(text)) !== null) {
        const v = m[0];
        if (!seenContact.has(v)) {
          facts.contacts.push({ type: 'email', value: v });
          seenContact.add(v);
        }
      }

      while ((m = phoneRx.exec(text)) !== null) {
        const v = m[0].trim();
        if (!seenContact.has(v)) {
          facts.contacts.push({ type: 'phone', value: v });
          seenContact.add(v);
        }
      }

      const actionLines = text.split(/\n+/).filter(l => /\b(pay|confirm|check[- ]?in|download|track|manage|reset|verify|complete|submit|reply)\b/i.test(l));
      facts.action_items = Array.from(new Set(actionLines.map(l => l.trim()).filter(l => l.length > 0 && l.length < 160))).slice(0, 6);

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

      return facts;
    } catch (err) {
      console.error('[EmailService._extractFacts]', err.message);
      return facts;
    }
  }

  static async _generateSummary(facts, snippet, metadata, model) {
    const intentCategories = [
      'Invoice', 'Receipt', 'Payment', 'Refund', 'Subscription',
      'Meeting Request', 'Calendar Invite', 'Reminder', 'Reschedule', 'Cancellation',
      'Flight Booking', 'Hotel Reservation', 'Travel Itinerary', 'Ticket', 'Reservation',
      'Order Confirmation', 'Shipping Update', 'Delivery Notice', 'Tracking', 'Return/Exchange',
      'Account Alert', 'Security Alert', 'Password Reset', 'Verification', 'Login Notification',
      'Task Assignment', 'Project Update', 'Status Report', 'Feedback Request', 'Approval Request',
      'Personal', 'Introduction', 'Follow-up', 'Thank You', 'Announcement',
      'Newsletter', 'Marketing', 'Promotion', 'Survey', 'Invitation',
      'Support Ticket', 'Bug Report', 'Feature Request', 'Complaint', 'Resolution',
      'Contract', 'Legal Notice', 'Policy Update', 'HR Notice', 'Compliance',
      'Bank Statement', 'Tax Document', 'Financial Report', 'Investment Update',
      'Social Notification', 'Connection Request', 'Mention', 'Comment',
      'Satire/Joke', 'Spam', 'Phishing Attempt', 'Auto-Reply', 'Out of Office',
      'Forwarded', 'Thread Reply', 'Digest', 'Notification', 'Other'
    ];

    const schema = {
      type: 'object',
      required: ['intent', 'reasoning', 'summary', 'action_items'],
      properties: {
        intent: {
          type: 'string',
          description: `Primary category. Choose the MOST specific match from: ${intentCategories.join(', ')}. If none fit exactly, use "Other".`
        },
        reasoning: {
          type: 'string',
          description: 'Brief analysis (1-2 sentences): tone (formal/casual/urgent/satirical), content type, any red flags (phishing indicators, spam patterns), and why you chose this intent.'
        },
        summary: {
          type: 'string',
          description: 'Clear, direct summary of core message in 1-2 sentences. Strip jargon, marketing fluff, legal boilerplate. For satirical/joke emails, describe the actual topic briefly.'
        },
        action_items: {
          type: 'array',
          items: { type: 'string' },
          description: 'Concrete actionable tasks only. Start each with a verb (Approve, Review, Pay, Confirm, Download, Reply, Schedule). Exclude vague items. Empty array if no actions needed.'
        },
        urgency: {
          type: 'string',
          enum: ['critical', 'high', 'normal', 'low', 'none'],
          description: 'How time-sensitive is this email? critical=immediate action, high=today, normal=this week, low=whenever, none=informational only'
        },
        key_details: {
          type: 'object',
          properties: {
            booking_reference: { type: 'string', description: 'Order ID, confirmation number, PNR, tracking number' },
            amount: { type: 'string', description: 'Total amount with currency if financial' },
            main_date: { type: 'string', description: 'Most important date/deadline' },
            dates: { type: 'array', items: { type: 'string' }, description: 'All relevant dates' },
            financials: { type: 'array', items: { type: 'string' }, description: 'All amounts mentioned' },
            sender_org: { type: 'string', description: 'Company/organization name if identifiable' }
          }
        }
      }
    };

    const factsText = this._buildFactsSummary(facts);

    let metadataCtx = '';
    if (metadata) {
      metadataCtx = 'EMAIL METADATA:\n';
      if (metadata.sender) metadataCtx += `From: ${metadata.sender}`;
      if (metadata.senderEmail) metadataCtx += ` <${metadata.senderEmail}>`;
      if (metadata.sender || metadata.senderEmail) metadataCtx += '\n';
      if (metadata.date) metadataCtx += `Sent: ${metadata.date}\n`;
      if (metadata.subject) metadataCtx += `Subject: ${metadata.subject}\n`;
      if (metadata.to) metadataCtx += `To: ${metadata.to}\n`;
      metadataCtx += '\n';
    }

    const systemPrompt = `You are an expert Executive Assistant who processes all types of emails. Your job is to filter noise, extract signal, and classify accurately.

CORE RULES:
1. BE SPECIFIC with intent - choose the most precise category (e.g., "Flight Booking" not just "Confirmation")
2. STRIP NOISE - ignore marketing fluff, legal disclaimers, unsubscribe footers, signature blocks
3. DETECT SATIRE/JOKES - if email uses absurd language, sci-fi jargon, or obvious humor, mark as "Satire/Joke" and explain the actual topic
4. FLAG SUSPICIOUS - for phishing/spam, note red flags in reasoning (urgency pressure, suspicious links, grammar errors, impersonation)
5. ACTION ITEMS must be concrete and start with a verb. Exclude vague suggestions.
6. URGENCY matters - deadlines, expiring offers, security alerts are high/critical. Newsletters are low/none.

SPECIAL CASES:
- Auto-replies/OOO: summarize return date and alternate contact
- Forwarded emails: focus on why it was forwarded, not just the original content
- Thread replies: focus on the new information, not repeated context
- Newsletters/Digests: extract only genuinely useful information
- Receipts/Invoices: always include amount and reference number
- Travel bookings: include dates, confirmation numbers, locations
- Security alerts: always mark as high/critical urgency

OUTPUT: JSON only. No explanations outside the schema.`;

    const userPrompt = `Analyze this email and classify it accurately.

${metadataCtx}---
EMAIL CONTENT:
${snippet}
---
EXTRACTED FACTS (use to verify details):
${factsText}
---

STEPS:
1. Identify the sender type (company, person, automated system)
2. Determine the primary PURPOSE of this email
3. Check for red flags (phishing, spam patterns, satire/jokes)
4. Extract concrete action items (if any)
5. Assess urgency based on deadlines and content type

Respond with JSON matching the schema. Be precise with intent classification.`;

    return await OllamaService.tryWithFallback(async (selectedModel) => {
      const result = await OllamaService.complete(
        selectedModel,
        [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: userPrompt }
        ],
        { format: schema, temperature: 0.1, top_p: 0.8 }
      );

      if (!result.ok) throw new Error(result.error);

      const parsed = JSON.parse(result.content);

      const mainDate = parsed.key_details?.main_date || (facts.dates?.[0]?.when);
      const bookingRef = parsed.key_details?.booking_reference || (facts.ids?.[0]?.value);
      const amount = parsed.key_details?.amount || (facts.amounts?.[0] ? `${facts.amounts[0].value} ${facts.amounts[0].currency || ''}` : null);

      const badPatterns = /^(whenever|maybe|consider|possibly|if you|you could)/i;
      const actionItems = (parsed.action_items || [])
        .filter(item => item && item.length > 3 && !badPatterns.test(item.trim()))
        .slice(0, 4);

      return {
        summary: parsed.summary || 'no summary generated',
        action_items: actionItems,
        dates: mainDate ? [mainDate] : [],
        key_facts: { 
          booking_reference: bookingRef, 
          amount,
          sender_org: parsed.key_details?.sender_org || null
        },
        intent: parsed.intent || null,
        reasoning: parsed.reasoning || null,
        urgency: parsed.urgency || 'normal'
      };
    }, 'email_summary');
  }

  static _buildFactsSummary(facts) {
    const lines = [];

    if (facts.amounts?.length > 0) {
      lines.push('amounts:');
      facts.amounts.forEach(a => lines.push(`- ${a.label}: ${a.value} ${a.currency || ''}`));
    }

    if (facts.ids?.length > 0) {
      lines.push('\nids:');
      facts.ids.forEach(id => lines.push(`- ${id.label}: ${id.value}`));
    }

    if (facts.dates?.length > 0) {
      lines.push('\ndates:');
      facts.dates.forEach(d => lines.push(`- ${d.label}: ${d.when}`));
    }

    if (facts.people?.length > 0) {
      lines.push('\npeople:');
      facts.people.forEach(p => lines.push(`- ${p}`));
    }

    if (facts.locations?.length > 0) {
      lines.push('\nlocations:');
      facts.locations.forEach(l => lines.push(`- ${l}`));
    }

    if (facts.action_items?.length > 0) {
      lines.push('\nactions:');
      facts.action_items.forEach(a => lines.push(`- ${a}`));
    }

    if (facts.contacts?.length > 0) {
      lines.push('\ncontacts:');
      facts.contacts.forEach(c => lines.push(`- ${c.type}: ${c.value}`));
    }

    if (facts.links?.length > 0) {
      lines.push('\nlinks:');
      facts.links.forEach(l => { if (l.url) lines.push(`- ${l.label}: ${l.url}`); });
    }

    return lines.join('\n') || 'no facts extracted';
  }
}
