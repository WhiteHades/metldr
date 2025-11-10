// page type detection and content extraction for smart summarization
// detects articles, docs, forums, academic papers
// extracts clean main content without nav/ads/sidebars

export class ContentDetector {
  constructor() {
    this.pageType = 'unknown';
    this.confidence = 0;
  }

  // returns { type, confidence, metadata }
  detectPageType() {
    const url = window.location.href;
    const hostname = window.location.hostname;
    const doc = document;

    const domainChecks = [
      { pattern: /mail\.google\.com/, type: 'email', confidence: 1.0 },
      { pattern: /(docs?\.|documentation|developer\.|api\.|guide\.)/, type: 'documentation', confidence: 0.9 },
      { pattern: /(reddit\.com|news\.ycombinator\.com|stackoverflow\.com)/, type: 'forum', confidence: 0.95 },
      { pattern: /(arxiv\.org|\.edu|scholar\.google)/, type: 'academic', confidence: 0.9 },
      { pattern: /(medium\.com|substack\.com|dev\.to|hashnode)/, type: 'article', confidence: 0.85 },
      { pattern: /github\.com.*\/(readme|blob|tree)/, type: 'documentation', confidence: 0.8 },
      { pattern: /wikipedia\.org/, type: 'encyclopedia', confidence: 1.0 }
    ];

    for (const check of domainChecks) {
      if (check.pattern.test(hostname) || check.pattern.test(url)) {
        this.pageType = check.type;
        this.confidence = check.confidence;
        return { 
          type: check.type, 
          confidence: check.confidence, 
          metadata: this.extractMetadata() 
        };
      }
    }

    const contentAnalysis = this.analyzeContent();
    this.pageType = contentAnalysis.type;
    this.confidence = contentAnalysis.confidence;

    return {
      type: contentAnalysis.type,
      confidence: contentAnalysis.confidence,
      metadata: this.extractMetadata()
    };
  }

  analyzeContent() {
    const doc = document;
    
    const hasArticleTag = !!doc.querySelector('article');
    const hasMainTag = !!doc.querySelector('main');
    const wordCount = this.getWordCount();
    const codeBlockRatio = this.getCodeBlockRatio();
    const headingCount = doc.querySelectorAll('h1, h2, h3').length;
    
    const ogType = doc.querySelector('meta[property="og:type"]')?.content;
    if (ogType === 'article') {
      return { type: 'article', confidence: 0.9 };
    }

    // check schema.org structured data
    const schemaArticle = doc.querySelector('script[type="application/ld+json"]');
    if (schemaArticle) {
      try {
        const schema = JSON.parse(schemaArticle.textContent);
        if (schema['@type'] === 'Article' || schema['@type'] === 'NewsArticle') {
          return { type: 'article', confidence: 0.95 };
        }
      } catch (e) {
        // ignore
      }
    }

    // documentation has high code ratio + many headings
    if (codeBlockRatio > 0.2 && headingCount > 5) {
      return { type: 'documentation', confidence: 0.75 };
    }

    if ((hasArticleTag || hasMainTag) && wordCount > 300) {
      return { type: 'article', confidence: 0.7 };
    }

    // forum detection via comment structure
    const commentSelectors = [
      '.comment', '[class*="comment"]', 
      '.reply', '[class*="reply"]',
      '[data-testid*="comment"]'
    ];
    const hasComments = commentSelectors.some(sel => doc.querySelectorAll(sel).length > 3);
    if (hasComments) {
      return { type: 'forum', confidence: 0.65 };
    }

    if (wordCount > 200) {
      return { type: 'article', confidence: 0.5 };
    }

    return { type: 'unknown', confidence: 0 };
  }

  // readability-like algorithm to extract main content
  extractContent() {
    const candidates = this.findContentCandidates();
    const best = this.selectBestCandidate(candidates);
    
    if (!best) {
      return { 
        content: document.body.innerText.slice(0, 10000), 
        sections: [] 
      };
    }

    const cleanContent = this.cleanContent(best);
    const sections = this.extractSections(best);

    return {
      content: cleanContent,
      sections: sections,
      element: best
    };
  }

  findContentCandidates() {
    const candidates = [];
    
    const semanticSelectors = [
      'article',
      'main',
      '[role="main"]',
      '.article-content',
      '.post-content',
      '.entry-content',
      '#content',
      '.content'
    ];

    for (const selector of semanticSelectors) {
      const elements = document.querySelectorAll(selector);
      elements.forEach(el => {
        const score = this.scoreElement(el);
        if (score > 0) {
          candidates.push({ element: el, score, selector });
        }
      });
    }

    if (candidates.length === 0) {
      const divs = document.querySelectorAll('div');
      divs.forEach(div => {
        const score = this.scoreElement(div);
        if (score > 20) {
          candidates.push({ element: div, score, selector: 'div' });
        }
      });
    }

    return candidates.sort((a, b) => b.score - a.score);
  }

  // score element as potential main content container
  // higher score = more likely to be article body
  scoreElement(element) {
    let score = 0;
    const text = element.innerText || '';
    const wordCount = text.trim().split(/\s+/).length;

    if (wordCount > 100) score += Math.min(wordCount / 10, 50);
    if (wordCount > 300) score += 20;
    if (wordCount > 1000) score += 10;

    const paragraphs = element.querySelectorAll('p');
    score += paragraphs.length * 3;

    const headings = element.querySelectorAll('h1, h2, h3, h4, h5, h6');
    score += headings.length * 5;

    const className = element.className?.toLowerCase() || '';
    const id = element.id?.toLowerCase() || '';
    const badPatterns = ['nav', 'menu', 'sidebar', 'footer', 'header', 'comment', 'ad', 'banner'];
    
    for (const pattern of badPatterns) {
      if (className.includes(pattern) || id.includes(pattern)) {
        score -= 20;
      }
    }

    // high link density = probably navigation
    const links = element.querySelectorAll('a');
    const linkTextLength = Array.from(links).reduce((sum, link) => sum + link.innerText.length, 0);
    const linkDensity = text.length > 0 ? linkTextLength / text.length : 0;
    if (linkDensity > 0.5) score -= 25;

    if (element.tagName === 'ARTICLE') score += 30;
    if (element.tagName === 'MAIN') score += 25;

    return score;
  }

  selectBestCandidate(candidates) {
    if (candidates.length === 0) return null;
    const best = candidates[0];
    return best.score > 20 ? best.element : null;
  }

  cleanContent(element) {
    const clone = element.cloneNode(true);
    
    const unwanted = [
      'script', 'style', 'noscript', 'iframe',
      'nav', 'header', 'footer', 'aside',
      '[class*="ad"]', '[class*="banner"]',
      '[class*="social"]', '[class*="share"]'
    ];

    unwanted.forEach(selector => {
      clone.querySelectorAll(selector).forEach(el => el.remove());
    });

    return clone.innerText.trim();
  }

  // extract h2/h3/h4 sections with content previews
  extractSections(element) {
    const sections = [];
    const headings = element.querySelectorAll('h1, h2, h3, h4');

    headings.forEach((heading, index) => {
      const level = parseInt(heading.tagName[1]);
      const title = heading.innerText.trim();
      
      let content = '';
      let currentNode = heading.nextSibling;
      
      while (currentNode) {
        if (currentNode.nodeType === Node.ELEMENT_NODE) {
          const tag = currentNode.tagName;
          if (/^H[1-6]$/.test(tag)) {
            const nextLevel = parseInt(tag[1]);
            if (nextLevel <= level) break;
          }
          content += currentNode.innerText + '\n';
        }
        currentNode = currentNode.nextSibling;
      }

      sections.push({
        title,
        content: content.trim().slice(0, 500),
        level,
        element: heading
      });
    });

    return sections;
  }

  extractMetadata() {
    return {
      title: this.extractTitle(),
      author: this.extractAuthor(),
      publishDate: this.extractDate(),
      description: this.extractDescription(),
      url: window.location.href,
      domain: window.location.hostname
    };
  }

  extractTitle() {
    const ogTitle = document.querySelector('meta[property="og:title"]')?.content;
    if (ogTitle) return ogTitle;

    const docTitle = document.title;
    if (docTitle) return docTitle;

    const h1 = document.querySelector('h1');
    return h1?.innerText || 'untitled';
  }

  extractAuthor() {
    const ogAuthor = document.querySelector('meta[property="article:author"]')?.content;
    if (ogAuthor) return ogAuthor;

    const authorMeta = document.querySelector('meta[name="author"]')?.content;
    if (authorMeta) return authorMeta;

    const schema = document.querySelector('script[type="application/ld+json"]');
    if (schema) {
      try {
        const data = JSON.parse(schema.textContent);
        if (data.author) return typeof data.author === 'string' ? data.author : data.author.name;
      } catch (e) {}
    }

    return null;
  }

  extractDate() {
    const ogDate = document.querySelector('meta[property="article:published_time"]')?.content;
    if (ogDate) return new Date(ogDate);

    const timeTag = document.querySelector('time[datetime]');
    if (timeTag) return new Date(timeTag.getAttribute('datetime'));

    return null;
  }

  extractDescription() {
    const ogDesc = document.querySelector('meta[property="og:description"]')?.content;
    if (ogDesc) return ogDesc;

    const metaDesc = document.querySelector('meta[name="description"]')?.content;
    return metaDesc || null;
  }

  getWordCount() {
    const text = document.body.innerText || '';
    return text.trim().split(/\s+/).length;
  }

  getCodeBlockRatio() {
    const allText = document.body.innerText.length;
    if (allText === 0) return 0;

    const codeElements = document.querySelectorAll('pre, code, .highlight');
    let codeTextLength = 0;
    
    codeElements.forEach(el => {
      codeTextLength += el.innerText.length;
    });

    return codeTextLength / allText;
  }

  isReadable() {
    const wordCount = this.getWordCount();
    const pageType = this.detectPageType();
    
    return wordCount > 200 && pageType.confidence > 0.5;
  }
}
