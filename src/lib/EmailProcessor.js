// Email processing pipeline - Ollama-powered
// Extracts key facts, generates structured summaries using Ollama
// Uses caching to avoid redundant API calls

export class EmailProcessor {
  constructor(ollamaClient, cacheManager, modelRouter) {
    this.client = ollamaClient;
    this.cache = cacheManager;
    this.router = modelRouter;
  }

  // generate email summary with structured output
  async summarizeEmail(emailContent, options = {}) {
    const {
      useCache = true,
      model = null
    } = options;

    // check cache first
    const emailId = this.generateEmailId(emailContent);
    
    if (useCache) {
      const cached = await this.cache.getEmailSummary(emailId);
      if (cached) {
        return cached.summary;
      }
    }

    // extract facts for context
    const EmailExtractor = (await import('./EmailExtractor.js')).EmailExtractor;
    const extracted = EmailExtractor.process(emailContent);

    // build focused prompt
    const prompt = this.buildSummaryPrompt(extracted);

    // select appropriate model
    const selectedModel = model || this.router.getModel('email_summary');

    // define clean JSON schema
    const schema = {
      "type": "object",
      "required": ["summary", "key_points", "action_items", "dates"],
      "properties": {
        "summary": { 
          "type": "string",
          "description": "One sentence overview of the email's main purpose"
        },
        "key_points": {
          "type": "array",
          "items": { "type": "string" },
          "description": "3-5 concise bullet points covering the main information",
          "minItems": 3,
          "maxItems": 5
        },
        "action_items": {
          "type": "array",
          "items": { "type": "string" },
          "description": "Specific tasks or requests mentioned in the email"
        },
        "dates": {
          "type": "array",
          "items": { "type": "string" },
          "description": "Important dates or deadlines mentioned"
        }
      }
    };

    // generate structured summary
    let summary = null;
    
    try {
      summary = await this.client.generateStructured(prompt, schema, {
        model: selectedModel,
        temperature: 0,  // Deterministic output
        system: "You are an expert email analyzer. Extract only information explicitly stated in the email. Do not infer or add information not present."
      });
    } catch (error) {
      console.error('Email summarization failed:', error);
      throw new Error(`Failed to summarize email: ${error.message}`);
    }

    // cache result
    if (useCache) {
      await this.cache.setEmailSummary(emailId, summary, {
        model: selectedModel,
        timestamp: Date.now()
      });
    }

    return summary;
  }

  // build concise, focused prompt
  buildSummaryPrompt(extracted) {
    const parts = [
      `Email from: ${extracted.sender}`,
      `Subject: ${extracted.subject}`,
      `\nContent:\n${extracted.body}`
    ];

    // Add context hints if available
    if (extracted.dates.length > 0) {
      parts.push(`\nDates found: ${extracted.dates.map(d => d.date).join(', ')}`);
    }

    return parts.join('\n');
  }



  // generate unique ID for email (for caching)
  generateEmailId(emailContent) {
    const subject = emailContent.match(/Subject:\s*(.+)/i)?.[1] || '';
    const sender = emailContent.match(/From:\s*([^\n]+)/i)?.[1] || '';
    const snippet = emailContent.substring(0, 500);
    
    // create hash
    const hash = this.simpleHash(subject + sender + snippet);
    return `email_${hash}`;
  }

  // simple string hash
  simpleHash(str) {
    let hash = 0;
    for (let i = 0; i < str.length; i++) {
      hash = ((hash << 5) - hash) + str.charCodeAt(i);
      hash = hash & hash; // convert to 32-bit integer
    }
    return Math.abs(hash).toString(36);
  }

  // test email processing
  async testProcessing(emailContent) {
    return await this.summarizeEmail(emailContent, { useCache: false });
  }
}

