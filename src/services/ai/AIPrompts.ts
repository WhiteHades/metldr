import type { EmailMetadata, ExtractedFacts, EmailSummary } from '../../types'

export interface EmailContext {
  metadata: EmailMetadata | null
  facts: ExtractedFacts | null
  summary: EmailSummary | null
  snippet: string
}

export interface PageChatContext {
  content: string
  title?: string
}

export const AIPrompts = {

  email: {
    summarySystem: `You are an Executive Assistant skilled at analyzing emails and extracting key information.
Respond ONLY with valid JSON. No markdown, no explanations.`,

    summaryUser: (snippet: string, metadataCtx: string, factsText: string): string => `Analyze this email and respond with JSON:
${metadataCtx}
EMAIL:
${snippet}

FACTS:
${factsText}

Respond with this exact JSON format:
{
  "intent": "category like Receipt/Invoice/Meeting/Travel/etc",
  "summary": "1-2 sentence summary of the email",
  "action_items": ["action 1", "action 2"],
  "urgency": "normal|high|low|critical",
  "key_details": { "amount": "$X", "date": "date if relevant" }
}`,

    replySystem: `You are an expert email assistant helping a busy professional respond quickly to emails.

CRITICAL - WHO TO ADDRESS:
- The "From" field shows WHO SENT the email TO the user
- Your replies are FROM the user TO the sender
- If email is FROM "Professor X", your reply should address "Professor X" (e.g., "Hi Professor X,")
- The "To" field is who RECEIVED the email (the user) - do NOT address them

CONTEXT AWARENESS:
1. Read the ORIGINAL EMAIL carefully - this is what you're replying to
2. Read the email summary and action items - your replies MUST ADDRESS these
3. If the email asks questions, ANSWER them directly in your replies
4. If the email requires confirmation, provide EXPLICIT confirmation
5. Reference SPECIFIC details from the email (dates, names, amounts, booking refs) when relevant
6. Match the sender's formality level (formal business → formal reply, casual → casual)

REPLY VARIETY:
- "Quick Confirm" (short): 1 sentence acknowledgment for info-only emails
- "Professional" (medium): Balanced formal reply addressing key points
- "Friendly" (medium): Warm, personable version with same substance
- "Detailed" (long): Thorough response covering ALL points (for complex emails)

QUALITY RULES:
1. Each reply MUST be immediately sendable with NO editing needed
2. NEVER include "Subject:" lines - these are REPLIES, not new emails
3. Do NOT use placeholders like "[your name]", "[meeting time]", or "[insert details]"
4. Do NOT make up information not in the email
5. Short replies should SKIP greetings/sign-offs entirely
6. Medium/long replies: greet the SENDER by name (from "From" field), not the recipient
7. For confirmations: use "Got it", "Confirmed", "Thanks for letting me know"
8. For questions: provide direct answers, not "I'll get back to you"
9. Reply body should be ONLY the message text - no headers, no subject lines

OUTPUT: JSON only. No explanations.`,

    replyUser: (ctx: EmailContext): string => {
      let metadataCtx = ''
      if (ctx.metadata) {
        const m = ctx.metadata
        const parts: string[] = []
        if (m.from) parts.push(`From: ${m.from}`)
        if (m.to) parts.push(`To: ${m.to}`)
        if (m.subject) parts.push(`Subject: ${m.subject}`)
        if (m.date) parts.push(`Date: ${m.date}`)
        metadataCtx = parts.join('\n') + '\n'
      }

      const summaryCtx = ctx.summary ? `
SUMMARY OF EMAIL:
- Intent: ${ctx.summary.intent || 'unknown'}
- Summary: ${ctx.summary.summary || ''}
- Action Items: ${(ctx.summary.action_items || []).join(', ') || 'none'}
- Urgency: ${ctx.summary.urgency || 'normal'}
` : ''

      return `Generate reply suggestions for this email.

IMPORTANT: You are writing replies TO the sender (shown in "From"). Address them, not the recipient.

${metadataCtx}${summaryCtx}
---
ORIGINAL EMAIL:
${ctx.snippet}
---

Generate 3-5 reply options with varying tones (professional/friendly/brief/detailed) and lengths (short/medium/long).`
    },

    replySchema: {
      type: 'object',
      required: ['replies'],
      properties: {
        replies: {
          type: 'array',
          items: {
            type: 'object',
            required: ['tone', 'length', 'body'],
            properties: {
              tone: { type: 'string', enum: ['professional', 'friendly', 'brief', 'detailed'] },
              length: { type: 'string', enum: ['short', 'medium', 'long'] },
              body: { type: 'string' },
              label: { type: 'string' }
            }
          },
          minItems: 3,
          maxItems: 5
        }
      }
    }
  },

  chat: {
    withContext: (ctx: PageChatContext): string => {
      const MAX_CONTEXT = 50000
      let content = ctx.content
      if (content.length > MAX_CONTEXT) {
        const headLen = Math.floor(MAX_CONTEXT * 0.6)
        const tailLen = MAX_CONTEXT - headLen
        content = content.slice(0, headLen) + 
          '\n\n[...content truncated for brevity...]\n\n' + 
          content.slice(-tailLen)
      }

      return `you are an assistant helping the user understand an article.

ARTICLE CONTENT:
${content}

RULES:
1. answer based ONLY on the article above
2. if info isn't in the article, say so
3. be concise (2-3 sentences unless more needed)`
    },

    noContext: 'you are a helpful assistant. be concise.'
  },

  page: {
    summarySystem: `you are a factual summariser. respond ONLY with bullet points.`,

    summaryUser: (snippet: string, title: string, author?: string): string => {
      let prefix = `Title: ${title}\n`
      if (author) prefix += `Author: ${author}\n`
      return `${prefix}\n${snippet}\n\nSummarize the above in 3 bullet points. Be factual and concise.`
    }
  },

  word: {
    defineSystem: 'you are a concise dictionary. respond in 15 words or less.',
    define: (word: string): string => `Define "${word}" briefly.`
  }
}
