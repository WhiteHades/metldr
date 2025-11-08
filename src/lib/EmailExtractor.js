// extract factual elements from email content
// dates, people, action items before AI processing
// reduces hallucination by providing structured facts to AI

export class EmailExtractor {
  // extract dates from email content
  static extractDates(text) {
    const dates = [];
    
    // ISO date format: YYYY-MM-DD
    const isoRegex = /(\d{4})-(\d{2})-(\d{2})/g;
    let match;
    while ((match = isoRegex.exec(text)) !== null) {
      dates.push({
        date: match[0],
        context: this.getContext(text, match.index, 50)
      });
    }

    // Common date formats
    const datePatterns = [
      /(\w+day),\s+(\d{1,2})\s+(\w+)\s+(\d{4})/g, // Monday, 15 January 2024
      /(\w+day),\s+(\d{1,2})\/(\d{1,2})\/(\d{4})/g, // Monday, 1/15/2024
      /(\d{1,2})\s+(January|February|March|April|May|June|July|August|September|October|November|December)\s+(\d{4})/gi,
      /(\d{1,2})\/(\d{1,2})\/(\d{4})/g // MM/DD/YYYY
    ];

    for (const pattern of datePatterns) {
      while ((match = pattern.exec(text)) !== null) {
        dates.push({
          date: match[0],
          context: this.getContext(text, match.index, 50)
        });
      }
    }

    // deduplicate
    return [...new Map(dates.map(d => [d.date, d])).values()];
  }

  // extract people (email addresses and names)
  static extractPeople(text) {
    const people = [];
    
    // email addresses
    const emailRegex = /([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})/g;
    let match;
    while ((match = emailRegex.exec(text)) !== null) {
      people.push({
        type: 'email',
        value: match[0],
        context: this.getContext(text, match.index, 50)
      });
    }

    // common name patterns (FROM: / TO: / CC: lines)
    const namePatterns = [
      /From:\s*([^\n]+)/g,
      /To:\s*([^\n]+)/g,
      /CC:\s*([^\n]+)/g,
      /BCC:\s*([^\n]+)/g
    ];

    for (const pattern of namePatterns) {
      while ((match = pattern.exec(text)) !== null) {
        people.push({
          type: 'name',
          value: match[1].trim(),
          context: match[0]
        });
      }
    }

    return [...new Map(people.map(p => [p.value, p])).values()];
  }

  // extract potential action items (lines with bullet points or explicit tasks)
  static extractActionItems(text) {
    const actions = [];
    
    const actionPatterns = [
      /[-•*]\s*(.+)/g, // bullet points
      /(?:please|need|should|must|will)\s+(.+?)[.!]/gi,
      /TODO:\s*(.+)/gi,
      /\b(action|todo|task):\s*(.+)/gi
    ];

    for (const pattern of actionPatterns) {
      let match;
      while ((match = pattern.exec(text)) !== null) {
        const item = match[1] || match[2];
        if (item && item.length < 200) {
          actions.push(item.trim());
        }
      }
    }

    return [...new Set(actions)];
  }

  // extract subject line
  static extractSubject(text) {
    const subjectMatch = text.match(/Subject:\s*(.+)/i);
    return subjectMatch ? subjectMatch[1].trim() : 'No subject';
  }

  // extract sender
  static extractSender(text) {
    const fromMatch = text.match(/From:\s*([^\n]+)/i);
    return fromMatch ? fromMatch[1].trim() : 'Unknown sender';
  }

  // get context around a match
  static getContext(text, index, chars) {
    const start = Math.max(0, index - chars);
    const end = Math.min(text.length, index + chars);
    return text.substring(start, end).trim();
  }

  // main extraction function
  static extractFacts(emailContent) {
    return {
      subject: this.extractSubject(emailContent),
      sender: this.extractSender(emailContent),
      dates: this.extractDates(emailContent),
      people: this.extractPeople(emailContent),
      actionItems: this.extractActionItems(emailContent),
      wordCount: emailContent.split(/\s+/).length
    };
  }

  // extract just the body (remove headers)
  static extractBody(emailContent) {
    const parts = emailContent.split(/\n\n/);
    
    // find first paragraph after headers
    let bodyStart = 0;
    const headerKeys = ['from:', 'to:', 'subject:', 'date:', 'cc:', 'bcc:'];
    
    for (let i = 0; i < parts.length; i++) {
      const line = parts[i].split('\n')[0].toLowerCase();
      if (!headerKeys.some(key => line.startsWith(key))) {
        bodyStart = i;
        break;
      }
    }

    return parts.slice(bodyStart).join('\n\n').trim();
  }

  // clean up email (remove signatures, quotes)
  static cleanEmail(emailContent) {
    // remove common signatures
    let cleaned = emailContent.replace(/--\s*[\s\S]*$/, '');
    
    // remove quoted replies (common patterns)
    cleaned = cleaned.replace(/^>.*$/gm, '');
    cleaned = cleaned.replace(/^On .+ wrote:.*$/gm, '');
    
    // remove excessive whitespace
    cleaned = cleaned.replace(/\n{3,}/g, '\n\n');
    
    return cleaned.trim();
  }

  // combine all extraction steps
  static process(emailContent) {
    const cleaned = this.cleanEmail(emailContent);
    const body = this.extractBody(cleaned);
    const facts = this.extractFacts(cleaned);

    return {
      ...facts,
      body,
      cleaned,
      hasActionItems: facts.actionItems.length > 0,
      hasDates: facts.dates.length > 0,
      hasMultiplePeople: facts.people.length > 1
    };
  }
}

