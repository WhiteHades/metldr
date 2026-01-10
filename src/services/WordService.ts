import { OllamaService } from './OllamaService'
import { dictionaryService } from './DictionaryService'
import { aiGateway } from './ai'
import type {
  Definition,
  WordLookupResult,
  LookupContext,
  ApiMeaning,
  ApiEntry,
  LLMParsedResult
} from '../types'

/**
 * word service with chrome ai integration
 * 
 * uses ai gateway for language detection with automatic fallback:
 * 1. regex pattern matching (fastest)
 * 2. chrome built-in language detector (if available)
 * 3. ollama local llm (fallback)
 */
export class WordService {
  static initialized = false
  
  static async ensureInit(): Promise<void> {
    if (!this.initialized) {
      try {
        await dictionaryService.init()
        this.initialized = true
      } catch (err) {
        console.warn('[WordService] dictionary init failed:', (err as Error).message)
      }
    }
  }
  
  static async lookup(word: string, context: LookupContext = {}): Promise<WordLookupResult | null> {
    await this.ensureInit()
    const word_lc = word.toLowerCase().trim()
    const encodedWord = encodeURIComponent(word_lc)

    console.log('[WordService] looking up:', word_lc, 'context:', context)

    const langs = context.languages || ['en']
    const isNonEnglish = (langs[0] && langs[0] !== 'en') || /[^\x00-\x7f]/.test(word)

    const tryLocal = async (): Promise<WordLookupResult | null> => {
      for (const langCode of langs) {
        try {
          const hasStore = await dictionaryService.hasLanguage(langCode)
          if (!hasStore) continue
          const localResult = await dictionaryService.find(word_lc, [langCode])
          if (localResult) {
            console.log('[WordService] found in local dict')
            return localResult
          }
        } catch (err) {
          console.error('[WordService] local tier error:', (err as Error).message)
        }
      }
      return null
    }

    const tryApi = async (): Promise<WordLookupResult | null> => {
      try {
        for (const langCode of langs) {
          try {
            const url = `https://api.dictionaryapi.dev/api/v2/entries/${langCode}/${encodedWord}`
            const res = await fetch(url)

            if (!res.ok) {
              console.log(`[WordService] api status ${res.status} for ${langCode}`)
              continue
            }

            const data = await res.json()
            if (data?.[0]?.meanings) {
              const result = this._formatFromApi(data[0] as ApiEntry, langCode)
              console.log('[WordService] found via API')
              return result
            }
          } catch (err) {
            console.log(`[WordService] api failed for ${langCode}:`, (err as Error).message)
            continue
          }
        }
      } catch (err) {
        console.log('[WordService] api tier error:', (err as Error).message)
      }
      return null
    }

    // english → local then api; non english → api then local. both end with ollama
    if (isNonEnglish) {
      const apiHit = await tryApi()
      if (apiHit) return apiHit
      const localHit = await tryLocal()
      if (localHit) return localHit
    } else {
      const localHit = await tryLocal()
      if (localHit) return localHit
      const apiHit = await tryApi()
      if (apiHit) return apiHit
    }

    try {
      const model = await OllamaService.getUserSelected() || await OllamaService.selectBest('word_lookup')
      if (!model) throw new Error('no models available')

      const result = await this._generateFromLLM(word_lc, context, model)
      console.log('[WordService] generated via LLM')
      return result
    } catch (err) {
      console.error('[WordService] llm tier error:', (err as Error).message)
    }

    return null
  }

  static _formatFromApi(entry: ApiEntry, langCode: string): WordLookupResult {
    const allDefs: Definition[] = []
    const posPriority = ['noun', 'verb', 'adjective', 'adverb']

    const sortedMeanings = [...entry.meanings].sort((a, b) => {
      const aIdx = posPriority.indexOf(a.partOfSpeech)
      const bIdx = posPriority.indexOf(b.partOfSpeech)
      if (aIdx === -1 && bIdx === -1) return 0
      if (aIdx === -1) return 1
      if (bIdx === -1) return -1
      return aIdx - bIdx
    })

    for (const meaning of sortedMeanings) {
      if (meaning.definitions?.length > 0) {
        for (const def of meaning.definitions) {
          const text = (def.definition || '').toLowerCase()

          if (text.includes('vulgar') ||
              text.includes('archaic') ||
              text.includes('obsolete') ||
              text.length < 10) {
            continue
          }

          allDefs.push({
            definition: def.definition,
            partOfSpeech: meaning.partOfSpeech || 'unknown',
            example: def.example || null,
            synonyms: def.synonyms || []
          })
        }
      }
    }

    if (allDefs.length === 0 && entry.meanings[0]) {
      const first = entry.meanings[0]
      allDefs.push({
        definition: first.definitions[0]?.definition || '',
        partOfSpeech: first.partOfSpeech || 'unknown',
        example: first.definitions[0]?.example || null,
        synonyms: first.definitions[0]?.synonyms || []
      })
    }

    const allSyns: string[] = []
    for (const meaning of entry.meanings || []) {
      for (const def of meaning.definitions || []) {
        if (def.synonyms && def.synonyms.length > 0) {
          allSyns.push(...def.synonyms)
        }
      }
    }
    const uniqueSyns = [...new Set(allSyns)].filter(s => s && s.toLowerCase() !== entry.word.toLowerCase())

    return {
      definitions: allDefs,
      synonyms: uniqueSyns,
      language: langCode,
      source: 'api'
    }
  }

  static async _generateFromLLM(word: string, context: LookupContext, model: string): Promise<WordLookupResult> {
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
    }

    const contextStr = context.fullSentence
      ? `Context: "${context.fullSentence}"`
      : (context.contextBefore || context.contextAfter
        ? `Context: "${context.contextBefore} [${word}] ${context.contextAfter}"`
        : '')

    const prompt = `${contextStr}

Define "${word}" (15-20 words max, considering context). Respond only with JSON.`

    const result = await OllamaService.complete(
      model,
      [
        { role: 'system', content: 'define words concisely. return valid json.' },
        { role: 'user', content: prompt }
      ],
      { format: schema, temperature: 0 }
    )

    if (!result.ok) throw new Error(result.error)

    try {
      const parsed: LLMParsedResult = JSON.parse(result.content || '{}')
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
      }
    } catch {
      return {
        definitions: [{
          definition: (result.content || '').trim(),
          partOfSpeech: 'unknown',
          example: null,
          synonyms: []
        }],
        synonyms: [],
        language: 'en',
        source: 'ollama'
      }
    }
  }

  /**
   * detect language using hybrid approach:
   * 1. regex patterns (fastest, for common scripts)
   * 2. chrome language detector api (high accuracy)
   * 3. ollama fallback (if chrome ai unavailable)
   */
  static async detectLanguage(word: string, sentence = ''): Promise<string> {
    const text = sentence || word

    // tier 1: regex pattern matching (fastest)
    const regexResult = this._detectLanguageByRegex(text)
    if (regexResult) {
      console.log('[WordService.detectLanguage] detected via regex:', regexResult)
      return regexResult
    }

    // tier 2: chrome language detector api
    try {
      const chromeResult = await aiGateway.detectLanguage({ text })
      if (chromeResult.ok && chromeResult.language) {
        console.log('[WordService.detectLanguage] detected via', chromeResult.provider, ':', chromeResult.language)
        return chromeResult.language
      }
    } catch (err) {
      console.log('[WordService.detectLanguage] ai gateway error:', (err as Error).message)
    }

    // tier 3: ollama fallback (handled by aiGateway internally)
    return 'en'
  }

  /**
   * fast regex-based language detection for common scripts
   */
  private static _detectLanguageByRegex(text: string): string | null {
    const patterns: Record<string, RegExp> = {
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
    }

    for (const [lang, pattern] of Object.entries(patterns)) {
      if (pattern.test(text)) return lang
    }

    return null
  }
}
