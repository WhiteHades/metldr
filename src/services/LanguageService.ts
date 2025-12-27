const SUPPORTED_LANGUAGES = ['en', 'es', 'ja'] as const
type SupportedLanguage = typeof SUPPORTED_LANGUAGES[number]

interface LanguageCache {
  language: SupportedLanguage
  timestamp: number
}

class LanguageServiceClass {
  private cache: Map<string, LanguageCache> = new Map()
  private readonly CACHE_TTL = 1000 * 60 * 30
  private detectorAvailable: boolean | null = null

  async isAvailable(): Promise<boolean> {
    if (this.detectorAvailable !== null) {
      return this.detectorAvailable
    }

    try {
      if (typeof LanguageDetector === 'undefined') {
        this.detectorAvailable = false
        return false
      }

      const avail = await LanguageDetector.availability()
      this.detectorAvailable = avail === 'available' || avail === 'downloadable'
      return this.detectorAvailable
    } catch {
      this.detectorAvailable = false
      return false
    }
  }

  async detect(text: string, cacheKey?: string): Promise<SupportedLanguage> {
    if (!text || text.length < 10) {
      return 'en'
    }

    if (cacheKey) {
      const cached = this.cache.get(cacheKey)
      if (cached && Date.now() - cached.timestamp < this.CACHE_TTL) {
        return cached.language
      }
    }

    try {
      if (!(await this.isAvailable())) {
        return 'en'
      }

      const detector = await LanguageDetector!.create()
      try {
        const sample = text.substring(0, 500)
        const results = await detector.detect(sample)

        if (results.length > 0) {
          const detected = results[0].detectedLanguage
          console.log('[LanguageService] detected:', detected, 'confidence:', results[0].confidence)

          const lang = this.mapToSupported(detected)

          if (cacheKey) {
            this.cache.set(cacheKey, { language: lang, timestamp: Date.now() })
          }

          return lang
        }
      } finally {
        detector.destroy()
      }
    } catch (e) {
      console.log('[LanguageService] detection failed:', (e as Error).message)
    }

    return 'en'
  }

  private mapToSupported(detected: string): SupportedLanguage {
    const normalized = detected.toLowerCase().split('-')[0]

    if (SUPPORTED_LANGUAGES.includes(normalized as SupportedLanguage)) {
      return normalized as SupportedLanguage
    }

    return 'en'
  }

  async detectForEmail(emailId: string, content: string): Promise<SupportedLanguage> {
    return this.detect(content, `email:${emailId}`)
  }

  async detectForPage(url: string, content: string): Promise<SupportedLanguage> {
    const urlKey = new URL(url).hostname + new URL(url).pathname
    return this.detect(content, `page:${urlKey}`)
  }

  async detectForWord(word: string): Promise<SupportedLanguage> {
    return this.detect(word)
  }

  getSupportedLanguages(): string[] {
    return [...SUPPORTED_LANGUAGES]
  }

  isSupported(lang: string): boolean {
    return SUPPORTED_LANGUAGES.includes(lang.toLowerCase().split('-')[0] as SupportedLanguage)
  }

  clearCache(): void {
    this.cache.clear()
  }
}

export const languageService = new LanguageServiceClass()
