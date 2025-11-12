import { OllamaService } from './OllamaService.js';
import { cacheService } from './CacheService.js';

export class PageService {
  static async summarize(content, pageType = 'article', metadata = {}, url = '', force = false) {
    try {
      if (!force && url) {
        const cached = await cacheService.getPageSummary(url);
        if (cached) return cached;
      }

      const schema = {
        type: 'object',
        required: ['bullets', 'confidence', 'readTime'],
        properties: {
          bullets: {
            type: 'array',
            items: { type: 'string' },
            description: 'exactly 3 clear bullet points'
          },
          confidence: {
            type: 'number',
            description: 'confidence 0-100'
          },
          readTime: {
            type: 'string',
            description: 'estimated read time (e.g. "5 min")'
          }
        }
      };

      const prompt = `summarize this ${pageType} in exactly 3 bullet points. respond only with json.

content:
${content}`;

      const model = await OllamaService.selectBest('page_summary');
      if (!model) throw new Error('no models available');

      const result = await OllamaService.complete(
        model,
        [{ role: 'user', content: prompt }],
        { format: schema, temperature: 0.3 }
      );

      if (!result.ok) throw new Error(result.error);

      const summary = JSON.parse(result.content);
      summary.pageType = pageType;
      summary.metadata = metadata;
      summary.timestamp = Date.now();

      if (url) {
        await cacheService.setPageSummary(url, summary, 3600);
      }

      return summary;
    } catch (err) {
      console.error('[PageService.summarize]', err.message);
      throw err;
    }
  }
}
