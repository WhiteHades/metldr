import { Readability } from '@mozilla/readability';

export class ArticleExtractor {
  static extract() {
    const startTime = performance.now();
    const url = window.location.href;
    const hostname = window.location.hostname;
    const contentType = document.contentType || 'text/html';
    const body = document.body;
    const bodyText = body?.innerText || '';
    const bodyWordCount = bodyText.split(/\s+/).filter(Boolean).length;
    const paragraphCount = document.querySelectorAll('p').length;
    const buttonCount = document.querySelectorAll('button').length;
    const inputCount = document.querySelectorAll('input, select, textarea').length;
    const formCount = document.querySelectorAll('form').length;
    const canvasCount = document.querySelectorAll('canvas').length;
    const svgCount = document.querySelectorAll('svg').length;
    const linkNodes = Array.from(document.querySelectorAll('a'));
    const linkTextLength = linkNodes.reduce((sum, node) => sum + (node.innerText || '').length, 0);
    const textLength = bodyText.length || 1;
    const linkDensity = linkTextLength / textLength;
    const buttonToParagraphRatio = paragraphCount > 0 ? (buttonCount + inputCount) / paragraphCount : buttonCount + inputCount;
    const hasArticleTag = !!document.querySelector('article');
    const hasMain = !!document.querySelector('main');
    const h1Count = document.querySelectorAll('h1').length;
    const hashRouting = window.location.hash?.includes('#/');
    const path = window.location.pathname || '';
    const isDashboard = /(dashboard|admin|console|panel)/i.test(path);
    const isSearch = /search/i.test(path) || /[?&](q|query|search)=/i.test(window.location.search);
    const isCart = /(cart|checkout)/i.test(path);
    const isFeed = /(\/feed|\/rss)/i.test(path);
    const isSocialFeed = /(twitter\.com|facebook\.com|instagram\.com|tiktok\.com|linkedin\.com)/i.test(hostname);
    const textDensity = textLength > 0 ? Math.min(1, textLength / Math.max(body?.innerHTML?.length || textLength, textLength)) : 0;
    const canvasHeavy = (canvasCount + svgCount) >= 3;
    const isSPA = hashRouting || buttonToParagraphRatio > 1.4 || formCount > paragraphCount * 0.6;
    
    if (/mail\.google\.com/.test(hostname)) {
      return { skip: true, reason: 'email client' };
    }
    
    if (/^chrome:|^edge:|^about:|^chrome-extension:/.test(url)) {
      return { skip: true, reason: 'system page' };
    }
    
    const documentClone = document.cloneNode(true) as Document
    const article = new Readability(documentClone, { charThreshold: 140 }).parse()
    
    if (!article?.textContent || article.textContent.length < 100) {
      return { 
        skip: true, 
        reason: 'not enough content',
        pageSignals: {
          isSPA,
          isDashboard,
          isSearch,
          isCart,
          isFeed,
          isSocialFeed,
          hasArticleTag,
          hasMain,
          h1Count,
          textDensity,
          linkDensity,
          buttonToParagraphRatio,
          canvasCount,
          svgCount,
          formCount,
          bodyWordCount,
          hashRouting,
          canvasHeavy
        },
        contentType
      };
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
      extractionTime,
      contentType,
      pageSignals: {
        isSPA,
        isDashboard,
        isSearch,
        isCart,
        isFeed,
        isSocialFeed,
        hasArticleTag,
        hasMain,
        h1Count,
        textDensity,
        linkDensity,
        buttonToParagraphRatio,
        canvasCount,
        svgCount,
        formCount,
        bodyWordCount,
        hashRouting,
        canvasHeavy
      }
    };
  }
}
