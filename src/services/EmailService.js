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
      }

      return summary;
    } catch (err) {
      console.error('[EmailService.summarize]', err.message);
      throw err;
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
    const schema = {
      type: 'object',
      required: ['summary', 'action_items'],
      properties: {
        summary: {
          type: 'string',
          description: '1-2 sentence summary with main action/event and key details'
        },
        action_items: {
          type: 'array',
          items: { type: 'string' },
          description: '3-4 concise actions for recipient'
        },
        key_details: {
          type: 'object',
          properties: {
            booking_reference: { type: 'string' },
            amount: { type: 'string' },
            main_date: { type: 'string' }
          }
        }
      }
    };

    const factsText = this._buildFactsSummary(facts);

    let metadataCtx = '';
    if (metadata) {
      metadataCtx = 'email metadata:\n';
      if (metadata.sender) metadataCtx += `from: ${metadata.sender}`;
      if (metadata.senderEmail) metadataCtx += ` <${metadata.senderEmail}>`;
      if (metadata.sender || metadata.senderEmail) metadataCtx += '\n';
      if (metadata.date) metadataCtx += `sent: ${metadata.date}\n`;
      if (metadata.subject) metadataCtx += `subject: ${metadata.subject}\n`;
      if (metadata.to) metadataCtx += `to: ${metadata.to}\n`;
      metadataCtx += '\n';
    }

    const systemPrompt = 'summarize emails concisely. be direct. skip meta phrases. write for recipient.';

    const userPrompt = `read this email and facts, create a brief summary:

${metadataCtx}email:
${snippet}

facts:
${factsText}

requirements:
- 1-2 sentences max
- lead with main action or event
- include key details (amounts, dates, ids) if relevant
- skip meta phrases
- be direct and scannable`;

    return await OllamaService.tryWithFallback(async (selectedModel) => {
      const result = await OllamaService.complete(
        selectedModel,
        [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: userPrompt }
        ],
        { format: schema, temperature: 0.3, top_p: 0.9 }
      );

      if (!result.ok) throw new Error(result.error);

      const parsed = JSON.parse(result.content);

      const mainDate = parsed.key_details?.main_date || (facts.dates?.[0]?.when);
      const bookingRef = parsed.key_details?.booking_reference || (facts.ids?.[0]?.value);
      const amount = parsed.key_details?.amount || (facts.amounts?.[0] ? `${facts.amounts[0].value} ${facts.amounts[0].currency || ''}` : null);

      const actionItems = (parsed.action_items || [])
        .filter(item => item && item.length > 3)
        .slice(0, 3);

      return {
        summary: parsed.summary || 'no summary generated',
        action_items: actionItems,
        dates: mainDate ? [mainDate] : [],
        key_facts: { booking_reference: bookingRef, amount }
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
