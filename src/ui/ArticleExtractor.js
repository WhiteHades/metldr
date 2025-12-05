import { Readability } from '@mozilla/readability';

export class ArticleExtractor {
  static extract() {
    const startTime = performance.now();
    const url = window.location.href;
    const hostname = window.location.hostname;
    
    if (/mail\.google\.com/.test(hostname)) {
      return { skip: true, reason: 'email client' };
    }
    
    if (/^chrome:|^edge:|^about:|^chrome-extension:/.test(url)) {
      return { skip: true, reason: 'system page' };
    }
    
    const documentClone = document.cloneNode(true);
    const article = new Readability(documentClone, { charThreshold: 100 }).parse();
    
    if (!article?.textContent || article.textContent.length < 100) {
      return { skip: true, reason: 'not enough content' };
    }
    
    const wordCount = article.textContent.split(/\s+/).filter(w => w).length;
    const extractionTime = Math.round(performance.now() - startTime);
    
    return {
      skip: false,
      title: article.title || document.title || 'untitled',
      content: article.textContent,
      excerpt: article.excerpt || '',
      author: article.byline || null,
      publication: article.siteName || hostname.replace('www.', ''),
      publishDate: article.publishedTime || null,
      url,
      domain: hostname.replace('www.', ''),
      wordCount,
      readTime: `${Math.max(1, Math.ceil(wordCount / 200))} min`,
      extractionTime
    };
  }
}
