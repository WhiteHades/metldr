// ocr service with chrome ai prompt api (primary) and tesseract-wasm (fallback)
// supports both window.ai.languageModel (2025+) and LanguageModel (older) patterns
// tesseract-wasm is lazy-loaded only when chrome ai unavailable

export interface OcrResult {
  text: string
  confidence: number
  source: 'chrome-ai' | 'tesseract'
}

// chrome ai types - both patterns
declare global {
  interface Window {
    // new pattern (2025+)
    ai?: {
      languageModel?: {
        capabilities: () => Promise<{ available: string }>
        create: (options?: any) => Promise<any>
      }
    }
    // older pattern (chrome_ai.md style)
    LanguageModel?: {
      availability: (options?: any) => Promise<string>
      create: (options?: any) => Promise<any>
    }
  }
}

class OcrService {
  private tesseractWorker: any = null
  private tesseractLoading = false
  
  // check chrome ai availability - tries both patterns
  async isChromeAIAvailable(): Promise<'new' | 'legacy' | false> {
    try {
      // try new pattern first: window.ai.languageModel
      if (window.ai?.languageModel) {
        const capabilities = await window.ai.languageModel.capabilities()
        if (capabilities.available === 'readily') return 'new'
      }
      
      // try legacy pattern: window.LanguageModel (chrome_ai.md style)
      if (window.LanguageModel) {
        const availability = await window.LanguageModel.availability({
          expectedInputs: [{ type: 'image' }]
        })
        if (availability === 'available' || availability === 'downloadable') return 'legacy'
      }
      
      return false
    } catch {
      return false
    }
  }

  // main ocr method - tries chrome ai first, falls back to tesseract
  async recognizeText(imageDataUrl: string): Promise<OcrResult> {
    console.log('[OcrService] Starting OCR...')
    
    const chromeAIMode = await this.isChromeAIAvailable()
    
    if (chromeAIMode) {
      try {
        const text = await this.ocrWithChromeAI(imageDataUrl, chromeAIMode)
        console.log(`[OcrService] Chrome AI OCR succeeded (${chromeAIMode} API)`)
        return { text, confidence: 0.9, source: 'chrome-ai' }
      } catch (err) {
        console.warn('[OcrService] Chrome AI OCR failed, falling back to tesseract:', err)
      }
    }
    
    // fallback to tesseract
    const text = await this.ocrWithTesseract(imageDataUrl)
    console.log('[OcrService] Tesseract OCR succeeded')
    return { text, confidence: 0.85, source: 'tesseract' }
  }

  // ocr using chrome ai - supports both api patterns
  private async ocrWithChromeAI(imageDataUrl: string, mode: 'new' | 'legacy'): Promise<string> {
    const imageBlob = await this.dataUrlToBlob(imageDataUrl)

    if (mode === 'new') {
      return await this.ocrWithNewAPI(imageBlob)
    } else {
      return await this.ocrWithLegacyAPI(imageBlob)
    }
  }

  // new api pattern: window.ai.languageModel
  private async ocrWithNewAPI(imageBlob: Blob): Promise<string> {
    if (!window.ai?.languageModel) throw new Error('Chrome AI not available')

    const session = await window.ai.languageModel.create({
      systemPrompt: 'You are an OCR engine. Extract all visible text from images exactly as it appears. Output only the extracted text with no explanations.',
      expectedInputs: [{ type: 'image' }]
    })

    try {
      const result = await session.prompt([
        'Extract all text from this image:',
        { type: 'image', content: imageBlob }
      ])
      return result || ''
    } finally {
      session.destroy?.()
    }
  }

  // legacy api pattern: window.LanguageModel (chrome_ai.md style)
  private async ocrWithLegacyAPI(imageBlob: Blob): Promise<string> {
    if (!window.LanguageModel) throw new Error('LanguageModel API not available')

    const session = await window.LanguageModel.create({
      expectedInputs: [
        { type: 'text', languages: ['en'] },
        { type: 'image' }
      ],
      expectedOutputs: [{ type: 'text', languages: ['en'] }]
    })

    try {
      const result = await session.prompt([
        {
          role: 'user',
          content: [
            { type: 'text', value: 'Extract all text from this image. Return only the extracted text, preserving paragraph structure.' },
            { type: 'image', value: imageBlob }
          ]
        }
      ])
      return result || ''
    } finally {
      session.destroy?.()
    }
  }

  private async dataUrlToBlob(dataUrl: string): Promise<Blob> {
    const response = await fetch(dataUrl)
    return await response.blob()
  }

  // tesseract fallback (lazy loaded)
  private async ocrWithTesseract(imageDataUrl: string): Promise<string> {
    if (!this.tesseractWorker) {
      await this.loadTesseract()
    }
    
    if (!this.tesseractWorker) throw new Error('Failed to load tesseract')
    
    const img = await this.loadImage(imageDataUrl)
    const canvas = document.createElement('canvas')
    canvas.width = img.width
    canvas.height = img.height
    const ctx = canvas.getContext('2d')!
    ctx.drawImage(img, 0, 0)
    const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height)
    
    const result = await this.tesseractWorker.recognize(imageData)
    return result.text
  }

  private async loadTesseract(): Promise<void> {
    if (this.tesseractLoading) {
      while (this.tesseractLoading) await new Promise(r => setTimeout(r, 100))
      return
    }
    
    this.tesseractLoading = true
    
    try {
      console.log('[OcrService] Loading tesseract-wasm...')
      const { createOCREngine } = await import('tesseract-wasm')
      
      const wasmUrl = chrome.runtime.getURL('assets/tesseract-core.wasm')
      const trainedDataUrl = chrome.runtime.getURL('assets/eng.traineddata')
      
      this.tesseractWorker = await createOCREngine({
        wasmBinary: await fetch(wasmUrl).then(r => r.arrayBuffer()),
        trainedData: await fetch(trainedDataUrl).then(r => r.arrayBuffer())
      })
      console.log('[OcrService] Tesseract loaded')
    } catch (err) {
      console.error('[OcrService] Failed to load tesseract:', err)
      throw err
    } finally {
      this.tesseractLoading = false
    }
  }

  private loadImage(dataUrl: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
      const img = new Image()
      img.onload = () => resolve(img)
      img.onerror = reject
      img.src = dataUrl
    })
  }

  dispose(): void {
    this.tesseractWorker?.destroy?.()
    this.tesseractWorker = null
  }
}

export const ocrService = new OcrService()
