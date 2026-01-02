import { OllamaService } from './OllamaService'
import { cacheService } from './CacheService'
import { aiGateway, AIPrompts } from './ai'
import type {
  AmountFact,
  IdFact,
  DateFact,
  ContactFact,
  LinkFact,
  ExtractedFacts,
  EmailMetadata,
  ReplySuggestion,
  EmailSummary,
  ParsedLLMSummary,
  ParsedReply,
  ParsedReplies
} from '../types'

import { ragService } from './rag/RagService'

// ... imports

export class EmailService {
  static async summarize(
    emailContent: string, 
    emailId: string | null = null, 
    metadata: EmailMetadata | null = null, 
    force = false
  ): Promise<EmailSummary> {
    const startTime = Date.now()
    
    if (emailId && emailContent) {
      ragService.indexChunks(emailContent, {
        sourceId: emailId,
        sourceUrl: `email://${emailId}`,
        sourceType: 'email',
        title: (metadata as any)?.subject || 'Email Thread'
      }).catch(err => console.warn('[EmailService] Chunk indexing failed', err))
    }

    try {
      if (force && emailId) {
// ... existing code ...
        try {
          await cacheService.deleteReplySuggestions(emailId)
        } catch (err) {
          console.warn('[EmailService.summarize] failed to clear reply suggestions:', (err as Error).message)
        }
      }

      if (!force && emailId) {
        const cached = await cacheService.getEmailSummary(emailId) as EmailSummary | null
        if (cached) {
          const elapsed = Date.now() - startTime          
          this._maybeGenerateReplies(emailId, emailContent, cached, metadata)
          return { ...cached, time_ms: elapsed, cached: true }
        }
      }

      const facts = this._extractFacts(emailContent)
      if (!facts || !Object.keys(facts).length) {
        throw new Error('no facts extracted')
      }

      const snippet = emailContent.length > 6000
        ? emailContent.substring(0, 4000) + '\n...[truncated]...\n' + emailContent.substring(emailContent.length - 2000)
        : emailContent

      // check provider preference
      const preferChrome = aiGateway.getPreference() === 'chrome-ai'
      
      // try preferred provider first for email summary
      if (preferChrome) {
        const chromeResult = await this._tryChromeSummary(snippet, metadata, facts)
        if (chromeResult) {
          const elapsed = Date.now() - startTime
          chromeResult.time_ms = elapsed
          chromeResult.cached = false
          chromeResult.model = 'gemini-nano'

          // fire-and-forget: enrich and generate replies in background (don't block response)
          if (emailId) {
            cacheService.setEmailSummary(emailId, chromeResult).catch(() => {})
            // run enrichment + replies in background
            this._enrichWithLocalModels(chromeResult, emailContent)
              .then(() => { if (emailId) cacheService.setEmailSummary(emailId, chromeResult) })
              .catch(() => {})
            this.generateReplySuggestions(emailId, emailContent, chromeResult, metadata).catch(() => {})
          }
          return chromeResult
        }
      }

      // use ollama (either as preference or fallback)
      const { available } = await OllamaService.checkAvailable()
      if (!available) {
        // if ollama was preferred but not available, try chrome as fallback
        if (!preferChrome) {
          const chromeResult = await this._tryChromeSummary(snippet, metadata, facts)
          if (chromeResult) {
            const elapsed = Date.now() - startTime
            chromeResult.time_ms = elapsed
            chromeResult.cached = false
            chromeResult.model = 'gemini-nano'

            // fire-and-forget: background tasks
            if (emailId) {
              cacheService.setEmailSummary(emailId, chromeResult).catch(() => {})
              this._enrichWithLocalModels(chromeResult, emailContent)
                .then(() => { if (emailId) cacheService.setEmailSummary(emailId, chromeResult) })
                .catch(() => {})
              this.generateReplySuggestions(emailId, emailContent, chromeResult, metadata).catch(() => {})
            }
            return chromeResult
          }
        }
        throw new Error('no ai available')
      }

      const model = await OllamaService.selectBest('email_summary')
      if (!model) throw new Error('no models available')

      const summary = await this._generateSummary(facts, snippet, metadata, model)

      const elapsed = Date.now() - startTime
      summary.time_ms = elapsed
      summary.cached = false
      summary.model = model

      if (emailId) {
        cacheService.setEmailSummary(emailId, summary).catch(() => {})
        this._enrichWithLocalModels(summary, emailContent)
          .then(() => { if (emailId) cacheService.setEmailSummary(emailId, summary) })
          .catch(() => {})
        this.generateReplySuggestions(emailId, emailContent, summary, metadata).catch(() => {})
      }

      return summary
    } catch (err) {
      console.error('[EmailService.summarize]', (err as Error).message)
      throw err
    }
  }

  static async _tryChromeSummary(snippet: string, metadata: EmailMetadata | null, facts: ExtractedFacts): Promise<EmailSummary | null> {
    try {
      const caps = await aiGateway.chrome.getCapabilities()
      if (!caps.complete) return null

      let metadataCtx = ''
      if (metadata) metadataCtx = this._buildMetadataContext(metadata)
      const factsText = this._buildFactsSummary(facts)

      const prompt = AIPrompts.email.summaryUser(snippet, metadataCtx, factsText)

      const result = await aiGateway.chrome.complete({
        systemPrompt: AIPrompts.email.summarySystem,
        userPrompt: prompt,
        temperature: 0.1
      })

      if (!result.ok) return null

      const jsonMatch = result.content?.match(/\{[\s\S]*\}/)
      if (!jsonMatch) return null

      const parsed = JSON.parse(jsonMatch[0])

      return {
        summary: parsed.summary || 'no summary',
        action_items: parsed.action_items || [],
        dates: parsed.key_details?.date ? [parsed.key_details.date] : [],
        key_facts: {
          booking_reference: null,
          amount: parsed.key_details?.amount || null,
          sender_org: null
        },
        intent: parsed.intent || 'Other',
        reasoning: null,
        urgency: parsed.urgency || 'normal'
      }
    } catch (err) {
      console.log('[EmailService._tryChromeSummary] failed:', (err as Error).message)
      return null
    }
  }

  // try chrome ai for reply generation using Writer API with summary context
  static async _tryChromReplies(
    snippet: string,
    summary: EmailSummary,
    metadata: EmailMetadata | null
  ): Promise<ReplySuggestion[] | null> {
    try {
      const caps = await aiGateway.chrome.getCapabilities()
      
      // prefer complete API for structured output
      if (caps.complete) {
        const emailContext = { metadata, facts: null, summary, snippet }
        const result = await aiGateway.chrome.complete({
          systemPrompt: AIPrompts.email.replySystem,
          userPrompt: AIPrompts.email.replyUser(emailContext),
          temperature: 0.4
        })

        if (!result.ok) return null

        // parse JSON from response
        const jsonMatch = result.content?.match(/\{[\s\S]*\}/)
        if (!jsonMatch) return null

        const parsed = JSON.parse(jsonMatch[0]) as ParsedReplies
        const replies = (parsed.replies || []).map((r, idx) => ({
          id: `reply_${idx}`,
          tone: r.tone || 'professional',
          length: r.length || 'medium',
          body: r.body || '',
          label: r.label || `Option ${idx + 1}`
        }))

        if (replies.length > 0) {
          console.log('[EmailService._tryChromReplies] generated', replies.length, 'replies via Chrome AI')
          return replies
        }
      }

      // fallback to Writer API if available (simpler but less structured)
      if (caps.write) {
        const context = this._buildReplyContext(snippet, summary, metadata)
        const suggestions: ReplySuggestion[] = []
        
        const tones: Array<{ tone: 'formal' | 'neutral' | 'casual'; label: string; length: 'short' | 'medium' | 'long' }> = [
          { tone: 'formal', label: 'Professional', length: 'medium' },
          { tone: 'casual', label: 'Friendly', length: 'medium' },
          { tone: 'neutral', label: 'Quick Reply', length: 'short' }
        ]

        for (const { tone, label, length } of tones) {
          const result = await aiGateway.chrome.write({
            prompt: `Write a ${label.toLowerCase()} reply to this email. Do NOT include subject lines. Email: ${snippet.substring(0, 1000)}`,
            context,
            tone,
            length
          })

          if (result.ok && result.content) {
            suggestions.push({
              id: `reply_${suggestions.length}`,
              tone: tone === 'formal' ? 'professional' : tone === 'casual' ? 'friendly' : 'brief',
              length,
              body: result.content,
              label
            })
          }
        }

        if (suggestions.length > 0) {
          console.log('[EmailService._tryChromReplies] generated', suggestions.length, 'replies via Writer API')
          return suggestions
        }
      }

      return null
    } catch (err) {
      console.log('[EmailService._tryChromReplies] failed:', (err as Error).message)
      return null
    }
  }

  // build context string for Writer API
  static _buildReplyContext(snippet: string, summary: EmailSummary, metadata: EmailMetadata | null): string {
    let context = ''
    if (metadata?.from) context += `From: ${metadata.from}\n`
    if (metadata?.subject) context += `Subject: ${metadata.subject}\n`
    if (summary.summary) context += `Summary: ${summary.summary}\n`
    if (summary.intent) context += `Intent: ${summary.intent}\n`
    if (summary.action_items?.length) context += `Action Items: ${summary.action_items.join(', ')}\n`
    return context
  }

  static async _maybeGenerateReplies(
    emailId: string, 
    emailContent: string, 
    summary: EmailSummary, 
    metadata: EmailMetadata | null
  ): Promise<void> {
    try {
      const cached = await cacheService.getReplySuggestions(emailId)
      if (!cached) {
        this._generateRepliesBackground(emailId, emailContent, summary, metadata)
      }
    } catch (err) {
      console.error('[EmailService._maybeGenerateReplies]', (err as Error).message)
    }
  }

  static _generateRepliesBackground(
    emailId: string, 
    emailContent: string, 
    summary: EmailSummary, 
    metadata: EmailMetadata | null
  ): void {
    this.generateReplySuggestions(emailId, emailContent, summary, metadata)
      .then(suggestions => {
        if (suggestions?.length > 0) {
          console.log('[EmailService] generated', suggestions.length, 'reply suggestions for', emailId)
        }
      })
      .catch(err => {
        console.error('[EmailService._generateRepliesBackground]', (err as Error).message)
      })
  }

  static async generateReplySuggestions(
    emailId: string, 
    emailContent: string, 
    summary: EmailSummary, 
    metadata: EmailMetadata | null = null
  ): Promise<ReplySuggestion[]> {
    try {
      const cached = await cacheService.getReplySuggestions(emailId) as ReplySuggestion[] | null
      if (cached) return cached

      // use full email content for better context-aware replies
      const snippet = emailContent

      const preferChrome = aiGateway.getPreference() === 'chrome-ai'

      // try preferred provider first
      if (preferChrome) {
        const chromeReplies = await this._tryChromReplies(snippet, summary, metadata)
        if (chromeReplies && chromeReplies.length > 0) {
          if (emailId) {
            await cacheService.setReplySuggestions(emailId, chromeReplies)
          }
          return chromeReplies
        }
      }

      // try ollama (either as preference or fallback)
      const { available } = await OllamaService.checkAvailable()
      if (available) {
        const model = await OllamaService.selectBest('email_summary')
        if (model) {
          const suggestions = await this._generateReplies(snippet, summary, metadata, model)
          
          if (suggestions?.length > 0 && emailId) {
            await cacheService.setReplySuggestions(emailId, suggestions)
          }
          return suggestions
        }
      }

      // if ollama was preferred but failed, try chrome as fallback
      if (!preferChrome) {
        const chromeReplies = await this._tryChromReplies(snippet, summary, metadata)
        if (chromeReplies && chromeReplies.length > 0) {
          if (emailId) {
            await cacheService.setReplySuggestions(emailId, chromeReplies)
          }
          return chromeReplies
        }
      }

      return []
    } catch (err) {
      console.error('[EmailService.generateReplySuggestions]', (err as Error).message)
      return []
    }
  }

  static async _generateReplies(
    snippet: string, 
    summary: EmailSummary | null, 
    metadata: EmailMetadata | null, 
    model: string
  ): Promise<ReplySuggestion[]> {
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
    }

    const emailContext = {
      metadata,
      facts: null,
      summary,
      snippet
    }

    const systemPrompt = AIPrompts.email.replySystem
    const userPrompt = AIPrompts.email.replyUser(emailContext)

    try {
      const result = await OllamaService.complete(
        model,
        [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: userPrompt }
        ],
        { format: schema, temperature: 0.4, top_p: 0.9 }
      )

      if (!result.ok) {
        console.error('[EmailService._generateReplies]', result.error)
        return []
      }

      const parsed: ParsedReplies = JSON.parse(result.content || '{}')
      return (parsed.replies || []).map((r, idx) => ({
        id: `reply_${idx}`,
        tone: r.tone || 'professional',
        length: r.length || 'medium',
        body: r.body || '',
        label: r.label || `Option ${idx + 1}`
      }))
    } catch (err) {
      console.error('[EmailService._generateReplies] parse error:', (err as Error).message)
      return []
    }
  }

  static async getCachedReplies(emailId: string): Promise<ReplySuggestion[] | null> {
    try {
      return await cacheService.getReplySuggestions(emailId) as ReplySuggestion[] | null
    } catch (err) {
      console.error('[EmailService.getCachedReplies]', (err as Error).message)
      return null
    }
  }

  static _extractFacts(text: string): ExtractedFacts {
    const facts: ExtractedFacts = {
      amounts: [],
      ids: [],
      dates: [],
      contacts: [],
      links: [],
      people: [],
      locations: [],
      action_items: []
    }

    try {
      const amountRx = /(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?\s?[$€£₹]?[\s]*\d{1,3}(?:[\s.,]\d{3})*(?:[.,]\d{2})?\s?(?:USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3})?/g
      const amountLabels = [/total/i, /amount/i, /price/i, /fare/i, /paid/i, /due/i]
      const seenAmounts = new Set<string>()
      let m: RegExpExecArray | null

      while ((m = amountRx.exec(text)) !== null) {
        const val = m[0].trim()
        if (val.length < 3) continue

        const ctx = text.substring(Math.max(0, m.index - 40), m.index + val.length + 10)
        const label = amountLabels.find(r => r.test(ctx))?.source.replace(/\//g, '') || 'amount'
        const currMatch = val.match(/USD|EUR|GBP|INR|BDT|HUF|AUD|CAD|CHF|JPY|CNY|HKD|[A-Z]{3}/)
        const curr = (currMatch ? currMatch[0] : null) || (val.includes('$') ? 'USD' : null)
        const key = `${label}:${val}:${curr || ''}`

        if (!seenAmounts.has(key)) {
          facts.amounts.push({ label, value: val.replace(/[^0-9.,]/g, ''), currency: curr })
          seenAmounts.add(key)
        }
      }

      const idPatterns = [
        /(booking|order|invoice|ticket|reference|ref|pnr|record locator)[:#]?\s*([A-Z0-9-]{5,})/gi,
        /(package|parcel)\s+number[:#]?\s*([A-Z0-9 \-]{6,})/gi,
        /(tracking|trace|parcel id|shipment id)[:#]?\s*([A-Z0-9-]{6,})/gi
      ]
      const seenIds = new Set<string>()

      for (const pattern of idPatterns) {
        while ((m = pattern.exec(text)) !== null) {
          const label = m[1].toLowerCase().replace(/\s+/g, '_')
          const value = m[2].replace(/\s{2,}/g, ' ').trim()
          const key = `${label}:${value}`

          if (!seenIds.has(key)) {
            facts.ids.push({ label, value })
            seenIds.add(key)
          }
        }
      }

      const isoDateRx = /\b\d{4}-\d{2}-\d{2}(?:[T\s]\d{2}:\d{2}(?::\d{2})?)?\b/g
      const humanDateRx = /(Mon|Tue|Wed|Thu|Fri|Sat|Sun)[a-z]*,?\s+\d{1,2}\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{4}(?:\s+\d{1,2}:\d{2}(?:\s*[AP]M)?\s*[A-Z]{2,3})?/gi
      const seenDates = new Set<string>()

      while ((m = isoDateRx.exec(text)) !== null) {
        const when = m[0]
        if (!seenDates.has(when)) {
          facts.dates.push({ label: 'date', when })
          seenDates.add(when)
        }
      }

      while ((m = humanDateRx.exec(text)) !== null) {
        const when = m[0]
        if (!seenDates.has(when)) {
          const ctx = text.substring(Math.max(0, m.index - 40), m.index + when.length + 10).toLowerCase()
          const label = /depart|departure|flight|outbound/.test(ctx) ? 'departure'
            : /arriv/.test(ctx) ? 'arrival'
            : /check[- ]?in/.test(ctx) ? 'check-in'
            : 'date'

          facts.dates.push({ label, when })
          seenDates.add(when)
        }
      }

      const emailRx = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g
      const phoneRx = /\+?\d[\d\s\-()]{6,}\d/g
      const seenContact = new Set<string>()

      while ((m = emailRx.exec(text)) !== null) {
        const v = m[0]
        if (!seenContact.has(v)) {
          facts.contacts.push({ type: 'email', value: v })
          seenContact.add(v)
        }
      }

      while ((m = phoneRx.exec(text)) !== null) {
        const v = m[0].trim()
        if (!seenContact.has(v)) {
          facts.contacts.push({ type: 'phone', value: v })
          seenContact.add(v)
        }
      }

      const actionLines = text.split(/\n+/).filter(l => /\b(pay|confirm|check[- ]?in|download|track|manage|reset|verify|complete|submit|reply)\b/i.test(l))
      facts.action_items = Array.from(new Set(actionLines.map(l => l.trim()).filter(l => l.length > 0 && l.length < 160))).slice(0, 6)

      const lines = text.split(/\n+/)
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i].trim()
        if (/^(z[- ]?box|locker|pick[- ]?up point)/i.test(line)) {
          const next = (lines[i + 1] || '').trim()
          const loc = [line.replace(/^z[- ]?box\s*/i, 'z-box'), next].filter(Boolean).join(', ')
          if (loc.length > 8 && !facts.locations.includes(loc)) {
            facts.locations.push(loc)
          }
        }
      }

      return facts
    } catch (err) {
      console.error('[EmailService._extractFacts]', (err as Error).message)
      return facts
    }
  }

  static async _generateSummary(
    facts: ExtractedFacts, 
    snippet: string, 
    metadata: EmailMetadata | null, 
    model: string
  ): Promise<EmailSummary> {
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
    ]

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
    }

    const factsText = this._buildFactsSummary(facts)

    let metadataCtx = ''
    if (metadata) {
      metadataCtx = this._buildMetadataContext(metadata)
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

OUTPUT: JSON only. No explanations outside the schema.`

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

Respond with JSON matching the schema. Be precise with intent classification.`

    return await OllamaService.tryWithFallback(async (selectedModel: string): Promise<EmailSummary> => {
      const result = await OllamaService.complete(
        selectedModel,
        [
          { role: 'system', content: systemPrompt },
          { role: 'user', content: userPrompt }
        ],
        { format: schema, temperature: 0.1, top_p: 0.8 }
      )

      if (!result.ok) throw new Error(result.error)

      const parsed: ParsedLLMSummary = JSON.parse(result.content || '{}')

      const mainDate = parsed.key_details?.main_date || (facts.dates?.[0]?.when)
      const bookingRef = parsed.key_details?.booking_reference || (facts.ids?.[0]?.value)
      const amount = parsed.key_details?.amount || (facts.amounts?.[0] ? `${facts.amounts[0].value} ${facts.amounts[0].currency || ''}` : null)

      const badPatterns = /^(whenever|maybe|consider|possibly|if you|you could)/i
      const actionItems = (parsed.action_items || [])
        .filter(item => item && item.length > 3 && !badPatterns.test(item.trim()))
        .slice(0, 4)

      return {
        summary: parsed.summary || 'no summary generated',
        action_items: actionItems,
        dates: mainDate ? [mainDate] : [],
        key_facts: { 
          booking_reference: bookingRef || null, 
          amount: amount || null,
          sender_org: parsed.key_details?.sender_org || null
        },
        intent: parsed.intent || null,
        reasoning: parsed.reasoning || null,
        urgency: parsed.urgency || 'normal'
      }
    }, 'email_summary')
  }

  static _buildFactsSummary(facts: ExtractedFacts): string {
    const lines: string[] = []

    if (facts.amounts?.length > 0) {
      lines.push('amounts:')
      facts.amounts.forEach(a => lines.push(`- ${a.label}: ${a.value} ${a.currency || ''}`))
    }

    if (facts.ids?.length > 0) {
      lines.push('\nids:')
      facts.ids.forEach(id => lines.push(`- ${id.label}: ${id.value}`))
    }

    if (facts.dates?.length > 0) {
      lines.push('\ndates:')
      facts.dates.forEach(d => lines.push(`- ${d.label}: ${d.when}`))
    }

    if (facts.people?.length > 0) {
      lines.push('\npeople:')
      facts.people.forEach(p => lines.push(`- ${p}`))
    }

    if (facts.locations?.length > 0) {
      lines.push('\nlocations:')
      facts.locations.forEach(l => lines.push(`- ${l}`))
    }

    if (facts.action_items?.length > 0) {
      lines.push('\nactions:')
      facts.action_items.forEach(a => lines.push(`- ${a}`))
    }

    if (facts.contacts?.length > 0) {
      lines.push('\ncontacts:')
      facts.contacts.forEach(c => lines.push(`- ${c.type}: ${c.value}`))
    }

    if (facts.links?.length > 0) {
      lines.push('\nlinks:')
      facts.links.forEach(l => { if (l.url) lines.push(`- ${l.label}: ${l.url}`) })
    }

    return lines.join('\n') || 'no facts extracted'
  }

  static _buildMetadataContext(metadata: EmailMetadata): string {
    if (!metadata) return ''

    const lines = ['EMAIL METADATA:']
    const fromName = metadata.from || metadata.sender || null
    const fromEmail = metadata.fromEmail || metadata.senderEmail || null
    const fromLine = [fromName, fromEmail ? `<${fromEmail}>` : null].filter(Boolean).join(' ').trim()
    if (fromLine) lines.push(`From: ${fromLine}`)

    const replyTo = metadata.replyTo || metadata['reply-to'] || null
    if (replyTo) lines.push(`Reply-To: ${replyTo}`)

    const toList = metadata.toList || metadata.toRecipients || null
    const to = metadata.to || (Array.isArray(toList) ? toList.join(', ') : null)
    if (to) lines.push(`To: ${to}`)

    const ccList = metadata.ccList || metadata.ccRecipients || null
    const cc = metadata.cc || (Array.isArray(ccList) ? ccList.join(', ') : null)
    if (cc) lines.push(`Cc: ${cc}`)

    const bccList = metadata.bccList || metadata.bccRecipients || null
    const bcc = metadata.bcc || (Array.isArray(bccList) ? bccList.join(', ') : null)
    if (bcc) lines.push(`Bcc: ${bcc}`)

    if (metadata.date) lines.push(`Date: ${metadata.date}`)
    if (metadata.subject) lines.push(`Subject: ${metadata.subject}`)
    if (metadata.mailedBy) lines.push(`Mailed-By: ${metadata.mailedBy}`)
    if (metadata.signedBy) lines.push(`Signed-By: ${metadata.signedBy}`)
    if (metadata.participants?.length) lines.push(`Participants: ${metadata.participants.join(', ')}`)
    if (metadata.emailCount) lines.push(`Message Count: ${metadata.emailCount}`)

    return lines.join('\n') + '\n\n'
  }

  // enrich email summary with local ML models (runs in parallel, non-blocking)
  static async _enrichWithLocalModels(summary: EmailSummary, emailContent: string): Promise<EmailSummary> {
    const snippet = emailContent.length > 1000 ? emailContent.slice(0, 1000) : emailContent
    
    try {
      // run classification and ner in parallel (sentiment model not bundled)
      const [classifyResult, nerResult] = await Promise.allSettled([
        aiGateway.classify(snippet, [
          'action-required', 'informational', 'scheduling', 
          'billing', 'shipping', 'inquiry', 'feedback', 'spam'
        ], true),
        aiGateway.extractEntities(snippet)
      ])

      // extract classification tags (top 3 with score > 0.3)
      if (classifyResult.status === 'fulfilled') {
        const { labels, scores } = classifyResult.value
        summary.tags = labels
          .map((label, i) => ({ label, score: scores[i] }))
          .filter(t => t.score > 0.3)
          .slice(0, 3)
      }

      // extract NER entities (people, organizations, dates)
      if (nerResult.status === 'fulfilled') {
        const entities = nerResult.value.entities
          .filter(e => ['PER', 'ORG', 'LOC', 'DATE', 'MISC'].includes(e.entity.replace('B-', '').replace('I-', '')))
          .slice(0, 10)
        summary.entities = entities.map(e => ({
          word: e.word,
          entity: e.entity.replace('B-', '').replace('I-', ''),
          score: e.score
        }))
      }

      // determine overall confidence
      const hasGoodTags = summary.tags && summary.tags.length > 0 && summary.tags[0].score > 0.5
      const hasEntities = summary.entities && summary.entities.length > 0
      summary.confidence = hasGoodTags && hasEntities ? 'high' : hasGoodTags || hasEntities ? 'medium' : 'low'

      console.log('[EmailService] Enriched with local models:', {
        tags: summary.tags?.length || 0,
        entities: summary.entities?.length || 0
      })
    } catch (err) {
      console.warn('[EmailService] Local model enrichment failed:', (err as Error).message)
      summary.confidence = 'low'
    }

    return summary
  }
}
