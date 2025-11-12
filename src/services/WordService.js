import { OllamaService } from './OllamaService.js';
import { dictionaryService } from './DictionaryService.js';

export class WordService {
  static async lookup(word, context = {}) {
    const word_lc = word.toLowerCase().trim();

    console.log('[WordService] looking up:', word_lc, 'context:', context);

    try {
      const langs = context.languages || ['en'];
      for (const langCode of langs) {
        try {
          const url = `https://api.dictionaryapi.dev/api/v2/entries/${langCode}/${word_lc}`;
          const res = await fetch(url);

          if (res.ok) {
            const data = await res.json();
            if (data?.[0]?.meanings) {
              const result = this._formatFromApi(data[0], langCode);
              console.log('[WordService] found via API');
              return result;
            }
          }
        } catch (err) {
          console.log(`[WordService] api failed for ${langCode}:`, err.message);
          continue;
        }
      }
    } catch (err) {
      console.log('[WordService] api tier error:', err.message);
    }

    try {
      const result = await dictionaryService.find(word_lc, context.languages);
      if (result) {
        console.log('[WordService] found in local dict');
        return result;
      }
    } catch (err) {
      console.error('[WordService] local tier error:', err.message);
    }

    try {
      const model = await OllamaService.selectBest('word_lookup');
      if (!model) throw new Error('no models available');

      const result = await this._generateFromLLM(word_lc, context, model);
      console.log('[WordService] generated via LLM');
      return result;
    } catch (err) {
      console.error('[WordService] llm tier error:', err.message);
    }

    return null;
  }

  static _formatFromApi(entry, langCode) {
    const allDefs = [];
    const posPriority = ['noun', 'verb', 'adjective', 'adverb'];

    const sortedMeanings = [...entry.meanings].sort((a, b) => {
      const aIdx = posPriority.indexOf(a.partOfSpeech);
      const bIdx = posPriority.indexOf(b.partOfSpeech);
      if (aIdx === -1 && bIdx === -1) return 0;
      if (aIdx === -1) return 1;
      if (bIdx === -1) return -1;
      return aIdx - bIdx;
    });

    for (const meaning of sortedMeanings) {
      if (meaning.definitions?.length > 0) {
        for (const def of meaning.definitions) {
          const text = (def.definition || '').toLowerCase();

          if (text.includes('vulgar') ||
              text.includes('archaic') ||
              text.includes('obsolete') ||
              text.length < 10) {
            continue;
          }

          allDefs.push({
            definition: def.definition,
            partOfSpeech: meaning.partOfSpeech || 'unknown',
            example: def.example || null,
            synonyms: def.synonyms || []
          });
        }
      }
    }

    if (allDefs.length === 0 && entry.meanings[0]) {
      const first = entry.meanings[0];
      allDefs.push({
        definition: first.definitions[0]?.definition,
        partOfSpeech: first.partOfSpeech || 'unknown',
        example: first.definitions[0]?.example || null,
        synonyms: first.definitions[0]?.synonyms || []
      });
    }

    const allSyns = [];
    for (const meaning of entry.meanings || []) {
      for (const def of meaning.definitions || []) {
        if (def.synonyms?.length > 0) {
          allSyns.push(...def.synonyms);
        }
      }
    }
    const uniqueSyns = [...new Set(allSyns)].filter(s => s && s.toLowerCase() !== entry.word.toLowerCase());

    return {
      definitions: allDefs,
      synonyms: uniqueSyns,
      language: langCode,
      source: 'api'
    };
  }

  static async _generateFromLLM(word, context, model) {
    const schema = {
      type: 'object',
      properties: {
        partOfSpeech: {
          type: 'string',
          enum: ['noun', 'verb', 'adjective', 'adverb', 'pronoun', 'preposition', 'conjunction', 'interjection', 'other']
        },
        definition: {
          type: 'string',
          description: 'concise 15-20 word definition'
        },
        example: {
          type: 'string',
          description: 'optional usage example'
        },
        synonyms: {
          type: 'array',
          items: { type: 'string' },
          description: '3-5 synonyms'
        }
      },
      required: ['partOfSpeech', 'definition']
    };

    const contextStr = context.fullSentence
      ? `Context: "${context.fullSentence}"`
      : (context.contextBefore || context.contextAfter
        ? `Context: "${context.contextBefore} [${word}] ${context.contextAfter}"`
        : '');

    const prompt = `${contextStr}

Define "${word}" (15-20 words max, considering context). Respond only with JSON.`;

    const result = await OllamaService.complete(
      model,
      [
        { role: 'system', content: 'define words concisely. return valid json.' },
        { role: 'user', content: prompt }
      ],
      { format: schema, temperature: 0 }
    );

    if (!result.ok) throw new Error(result.error);

    try {
      const parsed = JSON.parse(result.content);
      return {
        definitions: [{
          definition: parsed.definition || 'no definition',
          partOfSpeech: parsed.partOfSpeech || 'unknown',
          example: parsed.example || null,
          synonyms: parsed.synonyms || []
        }],
        synonyms: parsed.synonyms || [],
        language: 'en',
        source: 'ollama'
      };
    } catch (err) {
      console.error('[WordService] parse error:', err);
      return {
        definitions: [{
          definition: result.content.trim(),
          partOfSpeech: 'unknown',
          example: null,
          synonyms: []
        }],
        synonyms: [],
        language: 'en',
        source: 'ollama'
      };
    }
  }

  static async detectLanguage(word, sentence = '') {
    const patterns = {
      'de': /[äöüßÄÖÜ]/i,
      'fr': /[àâäéèêëïîôùûüÿç]/i,
      'es': /[ñáéíóúüÑÁÉÍÓÚ]/i,
      'it': /[àèéìíîòóùú]/i,
      'pt': /[àáâãéêíóôõúüç]/i,
      'ru': /[а-яё]/i,
      'ja': /[ひらがなカタカナ漢字]/i,
      'zh': /[一-龯]/i,
      'ko': /[가-힣]/i,
      'ar': /[ا-ي]/i,
      'he': /[א-ת]/i,
      'th': /[ก-๙]/i,
      'vi': /[àáạảãâầấậẩẫăằắặẳẵèéẹẻẽêềếệểễìíịỉĩòóọỏõôồốộổỗơờớợởỡùúụủũưừứựửữỳýỵỷỹđ]/i,
    };

    const text = sentence || word;
    for (const [lang, pattern] of Object.entries(patterns)) {
      if (pattern.test(text)) return lang;
    }

    try {
      const model = await OllamaService.selectBest('word_lookup');
      if (!model) return 'en';

      const schema = {
        type: 'object',
        properties: {
          language: {
            type: 'string',
            enum: ['en', 'de', 'fr', 'es', 'it', 'pt', 'ru', 'ja', 'zh', 'ko', 'ar', 'he', 'th', 'vi', 'other']
          }
        },
        required: ['language']
      };

      const result = await OllamaService.complete(
        model,
        [{ role: 'user', content: `Detect language of: "${text}". Respond only with JSON.` }],
        { format: schema, temperature: 0 }
      );

      if (result.ok) {
        try {
          const parsed = JSON.parse(result.content);
          const lang = parsed.language;
          if (lang && lang !== 'other') return lang;
        } catch (e) {
          console.log('[WordService.detectLanguage] parse error:', e);
        }
      }
    } catch (err) {
      console.log('[WordService.detectLanguage] llm error:', err.message);
    }

    return 'en';
  }
}
