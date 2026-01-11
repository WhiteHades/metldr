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

    replySystem: `You are an email assistant helping the USER (recipient) reply to emails they RECEIVED.

CRITICAL - PERSPECTIVE:
- "From" = the SENDER who wrote the email. You are writing a reply TO this person.
- "To" = the USER (recipient). The reply is FROM this person's perspective.
- YOU ARE THE RECIPIENT writing back to the sender.
- Example: Email FROM "Professor Smith" TO "john@example.com" → Your reply addresses "Professor Smith"

NEVER:
- Write as if you are the sender
- Address the recipient (that's the user!)
- Confuse who sent vs who received

CONTEXT AWARENESS:
1. Read the ORIGINAL EMAIL - this is what the sender wrote to the user
2. Your replies are the user's response TO that sender
3. If sender asks questions, the user's reply ANSWERS them
4. If sender requests confirmation, the user's reply CONFIRMS
5. Reference details from the sender's email (dates, names, amounts)
6. Match the sender's formality level

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

PERSPECTIVE: The user RECEIVED this email. You write replies FROM the user TO the sender.
- "From" = sender (address this person in replies)
- "To" = user/recipient (this is who is replying)

${metadataCtx}${summaryCtx}
---
ORIGINAL EMAIL (written by the sender):
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
