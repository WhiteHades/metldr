import { gsap } from 'gsap'
import { UIService } from './UIService'
import type { ContextData, LookupResponse, Definition } from '../types'

export class WordPopup {
  private container: HTMLElement | null = null
  private anchorRange: Range | null = null
  private scrollListener: (() => void) | null = null
  private resizeListener: (() => void) | null = null
  private clickListener: ((e: MouseEvent) => void) | null = null
  private isGmail: boolean
  private isCleaningUp = false

  constructor() {
    this.isGmail = window.location.hostname.includes('mail.google.com')
  }

  async handleTextSelection(e: MouseEvent): Promise<void> {
    if (this.isGmail || this.isCleaningUp) return

    if (this.container && this.container.contains(e.target as Node)) {
      return
    }

    const selection = window.getSelection()
    const selectedText = selection?.toString().trim() || ''

    if (this.container) {
      this.cleanup()
      await new Promise(resolve => setTimeout(resolve, 150))
    }

    if (!selectedText) return

    const wordCount = selectedText.split(/\s+/).length

    if (wordCount === 1) {
      const sel = window.getSelection()
      if (sel && sel.rangeCount > 0) {
        const range = sel.getRangeAt(0)
        const rect = range.getBoundingClientRect()

        const node = range.commonAncestorContainer
        const fullText = node.textContent || ''
        const offset = range.startOffset

        const sentenceEndChars = /[.!?\n]/
        let sentenceStart = 0
        let sentenceEnd = fullText.length

        for (let i = offset; i >= 0; i--) {
          if (sentenceEndChars.test(fullText[i]) || i === 0) {
            sentenceStart = i === 0 ? 0 : i + 1
            break
          }
        }

        for (let i = offset + selectedText.length; i < fullText.length; i++) {
          if (sentenceEndChars.test(fullText[i])) {
            sentenceEnd = i + 1
            break
          }
        }

        const fullSentence = fullText.substring(sentenceStart, sentenceEnd).trim()
        const contextBefore = fullText.substring(sentenceStart, offset).trim()
        const contextAfter = fullText.substring(offset + selectedText.length, sentenceEnd).trim()

        await this.show(selectedText, rect, { contextBefore, contextAfter, fullSentence })
      }
    }
  }

  updatePosition(): void {
    if (!this.container || !this.anchorRange) return

    try {
      const rect = this.anchorRange.getBoundingClientRect()
      const popupRect = this.container.getBoundingClientRect()

      const wordCenterX = rect.left + (rect.width / 2)
      const wordBottomY = rect.bottom

      let finalX = wordCenterX - (popupRect.width / 2)
      let finalY = wordBottomY + 12

      const viewportWidth = window.innerWidth
      const viewportHeight = window.innerHeight

      if (finalX < 10) {
        finalX = 10
      } else if (finalX + popupRect.width > viewportWidth - 10) {
        finalX = viewportWidth - popupRect.width - 10
      }

      if (finalY + popupRect.height > viewportHeight - 10) {
        finalY = rect.top - popupRect.height - 8
      }

      if (finalY < 10) {
        finalY = 10
      }

      this.container.style.position = 'fixed'
      this.container.style.top = finalY + 'px'
      this.container.style.left = finalX + 'px'
    } catch (error) {
      console.log('[popup] error updating position:', (error as Error).message)
    }
  }

  cleanup(): void {
    if (!this.container || this.isCleaningUp) return

    this.isCleaningUp = true

    this.container.style.pointerEvents = 'none'

    if (this.scrollListener) {
      window.removeEventListener('scroll', this.scrollListener, true)
      this.scrollListener = null
    }

    if (this.resizeListener) {
      window.removeEventListener('resize', this.resizeListener)
      this.resizeListener = null
    }

    if (this.clickListener) {
      document.removeEventListener('click', this.clickListener)
      this.clickListener = null
    }

    gsap.to(this.container, {
      opacity: 0,
      scale: 0.94,
      y: -4,
      duration: 0.12,
      ease: 'power2.in',
      onComplete: () => {
        if (this.container && this.container.parentNode) {
          this.container.remove()
        }
        this.container = null
        this.anchorRange = null
        this.isCleaningUp = false
      }
    })
  }

  async show(word: string, _selectionRect: DOMRect, contextData: ContextData = {}): Promise<void> {
    const settings = await chrome.storage.local.get(['wordPopupEnabled']) as { wordPopupEnabled?: boolean }
    if (settings.wordPopupEnabled === false) return

    const selection = window.getSelection()
    if (selection && selection.rangeCount > 0) {
      this.anchorRange = selection.getRangeAt(0).cloneRange()
    }

    this.container = document.createElement('div')
    this.container.className = 'metldr-inline-word-popup'

    this.container.style.cssText = `
      position: fixed;
      top: 0px;
      left: 0px;
      z-index: 999999;
      transform-origin: top left;
      visibility: hidden;
      will-change: opacity, transform;
      background: transparent;
      pointer-events: none;
    `

    const popup = document.createElement('div')
    popup.className = 'metldr-popup-body'

    const popupBg = UIService.currentTheme.bgSecondary

    popup.style.cssText = `
      --metldr-primary: ${UIService.currentTheme.primary};
      background: ${popupBg} !important;
      background-color: ${popupBg} !important;
      border: 1.5px solid ${UIService.currentTheme.border};
      border-radius: 12px;
      padding: 12px 16px;
      min-width: 240px;
      max-width: 360px;
      box-shadow: 0 8px 24px ${UIService.currentTheme.shadow}, 0 4px 12px ${UIService.currentTheme.shadow}, inset 0 1px 0 ${UIService.currentTheme.borderSubtle};
      font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Segoe UI', system-ui, sans-serif;
      will-change: opacity, transform;
      pointer-events: auto;
    `

    const header = this.createHeader()
    const content = this.createContent()
    const loader = this.createLoader()

    content.appendChild(loader)

    popup.appendChild(header)
    popup.appendChild(content)
    this.container.appendChild(popup)
    document.body.appendChild(this.container)

    UIService.updatePopupTheme(this.container)

    this.updatePosition()

    this.container.style.visibility = 'visible'

    gsap.fromTo(this.container,
      { opacity: 0, scale: 0.92, y: -6 },
      { opacity: 1, scale: 1, y: 0, duration: 0.16, ease: 'back.out(1.7)' }
    )

    this.scrollListener = () => this.updatePosition()
    this.resizeListener = () => this.updatePosition()
    this.clickListener = (e: MouseEvent) => {
      if (this.container && !this.container.contains(e.target as Node)) {
        this.cleanup()
      }
    }

    window.addEventListener('scroll', this.scrollListener, { passive: true })
    window.addEventListener('resize', this.resizeListener)

    setTimeout(() => {
      if (this.clickListener) document.addEventListener('click', this.clickListener)
    }, 100)

    await this.fetchAndRenderLookup(word, content, header, 'definition', contextData)
  }

  createHeader(): HTMLElement {
    const header = document.createElement('div')
    header.className = 'metldr-popup-header'
    header.style.cssText = `
      display: flex;
      align-items: center;
      gap: 6px;
      margin-bottom: 8px;
      flex-wrap: wrap;
    `
    return header
  }

  createContent(): HTMLElement {
    const content = document.createElement('div')
    content.className = 'metldr-popup-content'
    content.style.cssText = `
      font-size: 13px;
      color: ${UIService.currentTheme.text};
      line-height: 1.6;
      min-height: 20px;
      font-weight: 400;
      -webkit-font-smoothing: antialiased;
    `
    return content
  }

  createLoader(): HTMLElement {
    const loader = document.createElement('div')
    loader.style.cssText = 'display: flex; align-items: center; gap: 6px;'

    const spinner = document.createElement('div')
    spinner.style.cssText = `
      width: 12px;
      height: 12px;
      border: 2.5px solid ${UIService.currentTheme.border};
      border-top-color: ${UIService.currentTheme.primary};
      border-radius: 50%;
      animation: metldr-spin 0.6s linear infinite;
    `

    const loadText = document.createElement('span')
    loadText.textContent = 'looking up...'
    loadText.style.cssText = `
      color: ${UIService.currentTheme.textMuted};
      font-size: 11px;
      font-weight: 500;
    `

    loader.appendChild(spinner)
    loader.appendChild(loadText)

    return loader
  }

  async fetchAndRenderLookup(word: string, content: HTMLElement, header: HTMLElement, lookupType: string, contextData: ContextData = {}): Promise<void> {
    try {
      console.log('[popup] sending word lookup request:', word)
      
      let response: LookupResponse | undefined
      for (let attempt = 0; attempt < 3; attempt++) {
        response = await chrome.runtime.sendMessage({
          type: 'WORD_LOOKUP',
          word,
          lookupType,
          context: contextData
        }) as LookupResponse | undefined
        
        if (response !== undefined) break
        
        if (attempt < 2) {
          console.log('[popup] response undefined, retrying...', attempt + 1)
          await new Promise(r => setTimeout(r, 150))
        }
      }

      console.log('[popup] received response:', response)

      header.innerHTML = ''
      content.innerHTML = ''

      this.renderLookupResult(response, word, content, header, lookupType)

      this.updatePosition()
    } catch (error) {
      console.error('metldr: word lookup failed:', error)
      content.innerHTML = ''
      const errorText = document.createElement('span')
      errorText.textContent = 'lookup failed'
      errorText.style.cssText = `color: ${UIService.currentTheme.secondary || '#ff0080'}; font-size: 10px;`
      content.appendChild(errorText)

      this.updatePosition()
    }
  }

  renderLookupResult(response: LookupResponse | undefined, word: string, content: HTMLElement, header: HTMLElement, lookupType: string): void {
    if (!response || response.error) {
      this.renderError(word, response, header, content)
    } else if (response.result) {
      if (lookupType === 'definition') {
        this.renderDefinition(response, word, content, header)
      } else {
        this.renderTranslation(response, content, header)
      }
    }
  }

  renderError(word: string, response: LookupResponse | undefined, header: HTMLElement, content: HTMLElement): void {
    const errorText = document.createElement('span')
    errorText.textContent = response?.error || 'lookup failed'
    errorText.style.cssText = `
      color: ${UIService.currentTheme.secondary || '#ff0080'};
      font-size: 11px;
    `
    const wordSpan = document.createElement('span')
    wordSpan.style.cssText = `
      font-size: 14px;
      font-weight: 600;
      color: ${UIService.currentTheme.primary || '#00f0ff'};
      letter-spacing: 0.2px;
    `
    wordSpan.textContent = word
    header.appendChild(wordSpan)
    content.appendChild(errorText)
  }

  renderDefinition(response: LookupResponse, word: string, content: HTMLElement, header: HTMLElement): void {
    const synonymsList = response.result?.synonyms || []
    const sourceType = response.result?.source

    header.appendChild(this.createWordHeader(word, synonymsList, sourceType || ''))

    const definitions = response.result?.definitions || []

    if (definitions.length === 0) {
      const errorText = document.createElement('span')
      errorText.textContent = 'no definition found'
      errorText.style.cssText = `
        color: ${UIService.currentTheme.textMuted || '#888'};
        font-size: 11px;
      `
      content.appendChild(errorText)
    } else {
      const defsContainer = this.createDefinitionsContainer(definitions)
      content.appendChild(defsContainer)
    }
  }

  createWordHeader(word: string, synonymsList: string[], sourceType: string): HTMLElement {
    const headerContainer = document.createElement('div')
    headerContainer.style.cssText = `
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 6px;
    `

    const wordSpan = document.createElement('span')
    wordSpan.style.cssText = `
      font-size: 14px;
      font-weight: 700;
      color: ${UIService.currentTheme.primary || '#00f0ff'};
      letter-spacing: 0.2px;
    `
    wordSpan.textContent = word
    headerContainer.appendChild(wordSpan)

    if (synonymsList && synonymsList.length > 0) {
      this.appendSynonyms(headerContainer, synonymsList)
    }

    if (sourceType === 'ollama') {
      this.appendAiBadge(headerContainer)
    }

    return headerContainer
  }

  appendSynonyms(headerContainer: HTMLElement, synonymsList: string[]): void {
    const badgeStyle = this.getBadgeStyle()
    const visibleSyns = synonymsList.slice(0, 4)
    const hiddenSyns = synonymsList.slice(4)
    const hiddenCount = hiddenSyns.length

    visibleSyns.forEach((syn) => {
      const synBadge = document.createElement('span')
      synBadge.style.cssText = badgeStyle
      synBadge.textContent = syn.length > 11 ? syn.substring(0, 10) + '…' : syn
      headerContainer.appendChild(synBadge)
    })

    if (hiddenCount > 0) {
      const moreIndicator = document.createElement('span')
      moreIndicator.style.cssText = badgeStyle + `
        cursor: help;
        position: relative;
      `
      moreIndicator.textContent = `+${hiddenCount}`

      const tooltip = document.createElement('div')
      tooltip.style.cssText = `
        position: fixed;
        background: ${UIService.currentTheme.bgSecondary};
        color: ${UIService.currentTheme.text};
        padding: 8px 10px;
        border-radius: 5px;
        border: 1px solid ${UIService.currentTheme.border};
        font-size: 11px;
        white-space: normal;
        max-width: 200px;
        z-index: 2147483647;
        box-shadow: 0 8px 24px ${UIService.currentTheme.shadow};
        pointer-events: auto;
        opacity: 0;
        visibility: hidden;
        transition: opacity 150ms ease-out, visibility 150ms ease-out;
        font-weight: 600;
        line-height: 1.5;
        word-break: break-word;
      `
      tooltip.textContent = hiddenSyns.join(', ')
      document.body.appendChild(tooltip)

      moreIndicator.addEventListener('mouseenter', () => {
        const rect = moreIndicator.getBoundingClientRect()
        tooltip.style.left = (rect.right + 8) + 'px'
        tooltip.style.top = (rect.top - 8) + 'px'
        tooltip.style.opacity = '1'
        tooltip.style.visibility = 'visible'
      })

      moreIndicator.addEventListener('mouseleave', () => {
        tooltip.style.opacity = '0'
        tooltip.style.visibility = 'hidden'
      })

      headerContainer.appendChild(moreIndicator)
    }
  }

  appendAiBadge(headerContainer: HTMLElement): void {
    const badgeStyle = this.getBadgeStyle()
    const sourceHint = document.createElement('span')
    sourceHint.setAttribute('title', 'ai generated definition')
    sourceHint.style.cssText = badgeStyle + `
      cursor: help;
    `
    sourceHint.textContent = 'ai'
    headerContainer.appendChild(sourceHint)
  }

  getBadgeStyle(): string {
    return `
      font-size: 11px;
      color: ${UIService.currentTheme.primary || '#00f0ff'};
      padding: 3px 7px;
      background: ${UIService.currentTheme.border || 'rgba(255,255,255,0.08)'};
      border-radius: 4px;
      font-weight: 700;
      border: 1px solid ${UIService.currentTheme.border || 'rgba(255,255,255,0.15)'};
      opacity: 1;
      white-space: nowrap;
      display: inline-flex;
      align-items: center;
      justify-content: center;
    `
  }

  createDefinitionsContainer(definitions: Definition[]): HTMLElement {
    const defsContainer = document.createElement('div')
    defsContainer.className = 'metldr-definitions-scroll'
    defsContainer.style.cssText = `
      max-height: 280px;
      overflow-y: auto;
      margin: 0;
      padding-right: 4px;
      background: ${UIService.currentTheme.bgSecondary} !important;
      background-color: ${UIService.currentTheme.bgSecondary} !important;
      border-radius: 8px;
    `

    defsContainer.addEventListener('scroll', (e) => {
      e.stopPropagation()
    }, true)

    definitions.forEach((def, index) => {
      const defBlock = this.createDefinitionBlock(def, index, definitions.length)
      defsContainer.appendChild(defBlock)
    })

    return defsContainer
  }

  createDefinitionBlock(def: Definition, index: number, totalDefs: number): HTMLElement {
    const defBlock = document.createElement('div')
    defBlock.setAttribute('data-element-type', 'def-block')
    defBlock.style.cssText = `
      background: ${UIService.currentTheme.bgSecondary} !important;
      background-color: ${UIService.currentTheme.bgSecondary} !important;
      margin-bottom: ${index < totalDefs - 1 ? '12px' : '0'};
      padding: 8px 0;
      padding-bottom: ${index < totalDefs - 1 ? '12px' : '0'};
      border-bottom: ${index < totalDefs - 1 ? `1px solid ${UIService.currentTheme.border || 'rgba(255,255,255,0.1)'}` : 'none'};
    `

    const posTag = document.createElement('div')
    posTag.setAttribute('data-element-type', 'pos')
    posTag.style.cssText = `
      font-size: 9px;
      color: ${UIService.currentTheme.secondary || '#ff0080'};
      text-transform: uppercase;
      letter-spacing: 0.5px;
      font-weight: 600;
      margin-bottom: 4px;
    `
    posTag.textContent = def.partOfSpeech || 'unknown'
    defBlock.appendChild(posTag)

    const defText = document.createElement('div')
    defText.setAttribute('data-element-type', 'definition')
    defText.style.cssText = `
      font-size: 13px;
      line-height: 1.55;
      color: ${UIService.currentTheme.text || '#e0e0e0'};
      margin: 0;
    `
    defText.textContent = def.definition
    defBlock.appendChild(defText)

    if (def.example) {
      const exampleText = document.createElement('div')
      exampleText.style.cssText = `
        font-size: 11px;
        line-height: 1.5;
        color: ${UIService.currentTheme.textMuted || '#888'};
        margin-top: 4px;
        font-style: italic;
      `
      exampleText.textContent = `"${def.example}"`
      defBlock.appendChild(exampleText)
    }

    return defBlock
  }

  renderTranslation(response: LookupResponse, content: HTMLElement, header: HTMLElement): void {
    const wordSpan = document.createElement('span')
    wordSpan.style.cssText = `
      font-size: 14px;
      font-weight: 600;
      color: ${UIService.currentTheme.primary || '#00f0ff'};
      letter-spacing: 0.2px;
    `
    wordSpan.textContent = response.result?.word || 'translation'
    header.appendChild(wordSpan)

    const langInfo = document.createElement('p')
    langInfo.textContent = `${response.result?.sourceLang || 'unknown'} → ${response.result?.targetLang || 'english'}`
    langInfo.style.cssText = `
      margin: 0 0 6px 0;
      font-size: 9px;
      color: ${UIService.currentTheme.textMuted || '#888'};
      text-transform: uppercase;
    `

    const trans = document.createElement('p')
    trans.textContent = response.result?.translation || 'translation unavailable'
    trans.style.cssText = `
      margin: 0;
      font-weight: 600;
      color: ${UIService.currentTheme.primary || '#00f0ff'};
    `

    content.appendChild(langInfo)
    content.appendChild(trans)
  }
}
