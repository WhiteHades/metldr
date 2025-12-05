import { OllamaService } from './OllamaService.js';
import { cacheService } from './CacheService.js';

export class PageService {

  static async summarize(extractedData, force = false) {
    const startTotal = performance.now();
    const timing = { extraction: 0, llm: 0, total: 0 };
    const { title, url, content, author, publishDate, publication, wordCount, readTime, extractionTime } = extractedData;    
    timing.extraction = extractionTime || 0;
    
    if (!force && url) {
      const cached = await cacheService.getPageSummary(url);
      if (cached) {
        cached.timing = { ...cached.timing, cached: true, total: Math.round(performance.now() - startTotal) };
        return cached;
      }
    }

    let metadata = `ARTICLE METADATA:\n- Title: "${title}"\n`;
    if (author) metadata += `- Author: ${author}\n`;
    if (publication) metadata += `- Publication: ${publication}\n`;
    if (publishDate) metadata += `- Published: ${publishDate}\n`;
    metadata += '\n---\nARTICLE CONTENT:\n\n';

    const fullContent = metadata + content;

    const model = await OllamaService.selectBest('page_summary');
    if (!model) throw new Error('no models available');

    const llmStart = performance.now();
    const result = await OllamaService.complete(
      model,
      [
        { role: 'system', content: `you are a factual summariser. extract the most important points from articles.

RULES:
1. only use information explicitly stated in the article
2. never infer or add information not in the text
3. include specific names, numbers, dates exactly as stated
4. output exactly 3 bullet points, each starting with "- "
5. each bullet should be specific, not vague` },
        { role: 'user', content: `${fullContent}\n---\n\nWrite 3 bullet points summarising the key facts:` }
      ],
      { temperature: 0.1 }
    );
    timing.llm = Math.round(performance.now() - llmStart);

    if (!result.ok) throw new Error(result.error);

    const bullets = result.content
      .split('\n')
      .map(l => l.trim())
      .filter(l => /^[-•]|^\d+\./.test(l))
      .map(l => l.replace(/^[-•]\s*/, '').replace(/^\d+\.\s*/, ''))
      .filter(l => l.length > 10)
      .slice(0, 3);

    timing.total = Math.round(performance.now() - startTotal);
    timing.cached = false;
    timing.model = model;

    const summary = {
      title, author, publishDate, publication,
      bullets: bullets.length ? bullets : ['could not generate summary'],
      readTime, fullContent, wordCount,
      timestamp: Date.now(),
      timing
    };

    if (url) await cacheService.setPageSummary(url, summary, 3600);
    return summary;
  }

  static async chat(messages, pageContext, model) {
    const startTime = performance.now();
    
    if (!model) model = await OllamaService.selectBest('page_summary');
    
    if (!model) {
      console.error('[PageService.chat] no model available');
      return { ok: false, error: 'no models available' };
    }

    let contextText = pageContext?.fullContent || '';

    const MAX_CONTEXT = 50000;
    if (contextText.length > MAX_CONTEXT) {
      const headLen = Math.floor(MAX_CONTEXT * 0.6);
      const tailLen = MAX_CONTEXT - headLen;
      contextText = contextText.slice(0, headLen) + 
        '\n\n[...content truncated for brevity...]\n\n' + 
        contextText.slice(-tailLen);
    }

    const systemPrompt = contextText 
      ? `you are an assistant helping the user understand an article.\n\nARTICLE CONTENT:\n${contextText}\n\nRULES:\n1. answer based ONLY on the article above\n2. if info isn't in the article, say so\n3. be concise (2-3 sentences unless more needed)`
      : 'you are a helpful assistant. be concise.';

    const useLongTimeout = contextText.length > 2000;

    try {
      const result = await OllamaService.complete(
        model,
        [{ role: 'system', content: systemPrompt }, ...messages.slice(-6)],
        { temperature: 0.2, longContext: useLongTimeout }
      );
      
      result.timing = {
        total: Math.round(performance.now() - startTime),
        model
      };
      
      return result;
    } catch (err) {
      console.error('[PageService.chat] error:', err.message);
      return { ok: false, error: err.message || 'chat failed' };
    }
  }
}
