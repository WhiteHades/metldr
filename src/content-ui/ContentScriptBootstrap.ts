import { UIService } from './UIService'
import { WordPopup } from './WordPopup'
import { PdfToolbar } from './PdfToolbar'
import { emailExtractor } from './EmailExtractor'
import { ArticleExtractor } from './ArticleExtractor'
import type { ThemeColors } from '../types'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type InboxSDK = any

export class ContentScriptBootstrap {
  private static initialized = false

  static async init(sdk: InboxSDK | null = null): Promise<void> {
    // prevent double initialization
    if (this.initialized) return
    this.initialized = true

    const isGmail = window.location.hostname.includes('mail.google.com')

    UIService.init()
    await UIService.loadFromStorage()

    this.setupThemeListeners()
    this.setupMessageListeners()

    const popupManager = new WordPopup()
    document.addEventListener('dblclick', (e) => popupManager.handleTextSelection(e))

    if (isGmail && sdk) {
      this.initInboxSDK(sdk)
    } else if (isGmail) {
      console.log('metldr: gmail mode, waiting for inboxsdk')
    } else {
      console.log('metldr: non-gmail mode, word popup + pdf toolbar')
      // pdf toolbar auto-initializes on pdf pages
      new PdfToolbar()
    }
  }

  // separate method to init InboxSDK after base init
  static initInboxSDK(sdk: InboxSDK): void {
    if (!sdk) return
    emailExtractor.init(sdk, () => {})
    console.log('metldr: inboxsdk email extractor initialized')
  }

  static setupMessageListeners(): void {
    chrome.runtime.onMessage.addListener((msg, _sender, respond) => {
      if (msg.type === 'EXTRACT_ARTICLE') {
        try {
          const extracted = ArticleExtractor.extract()
          respond({ success: true, data: extracted })
        } catch (err) {
          console.error('metldr: extraction failed:', err)
          respond({ success: false, error: (err as Error).message })
        }
        return true
      }
      
      // handler for background to request email content
      if (msg.type === 'GET_EMAIL_CONTENT') {
        try {
          // try InboxSDK extraction first
          const extracted = emailExtractor.getLastExtracted()
          if (extracted) {
            respond({
              success: true,
              content: extracted.content,
              metadata: extracted.metadata
            })
            return true
          }
          
          const emailContent = ContentScriptBootstrap.extractEmailFromDOM()
          if (emailContent) {
            respond({
              success: true,
              content: emailContent.content,
              metadata: emailContent.metadata
            })
          } else {
            respond({ success: false, error: 'no email content found' })
          }
        } catch (err) {
          console.error('metldr: email content extraction failed:', err)
          respond({ success: false, error: (err as Error).message })
        }
        return true
      }
      
      return false
    })
  }

  static setupThemeListeners(): void {
    UIService.onChange((_themeName, _themeObj) => {
      console.log('metldr: theme changed to:', _themeName)
      UIService.updatePopupTheme(document.querySelector('.metldr-inline-word-popup'))
      UIService.updateSummaryTheme()
    })

    chrome.storage.onChanged.addListener((changes, area) => {
      if (area === 'local' && changes.theme) {
        const themeName = changes.theme.newValue as string
        UIService.setTheme(themeName)
      }
    })
  }

  // fallback DOM extraction for when InboxSDK hasn't initialized
  static extractEmailFromDOM(): { content: string; metadata: { subject?: string; from?: string } } | null {
    try {
      // gmail subject selector
      const subjectEl = document.querySelector('h2[data-thread-perm-id]') || 
                        document.querySelector('.hP') ||
                        document.querySelector('[role="heading"][data-thread-perm-id]')
      const subject = subjectEl?.textContent?.trim() || ''
      
      // gmail sender - look for sender element  
      const senderEl = document.querySelector('.gD') ||
                       document.querySelector('[email]') ||
                       document.querySelector('.go')
      const from = senderEl?.getAttribute('email') || senderEl?.textContent?.trim() || ''
      
      // gmail message bodies - multiple selectors for different views
      const messageContainers = document.querySelectorAll('.a3s.aiL, .ii.gt, [data-message-id] .a3s')
      
      if (messageContainers.length === 0) {
        console.log('metldr: no email bodies found in DOM')
        return null
      }
      
      let content = ''
      messageContainers.forEach((container, i) => {
        const text = container.textContent?.trim() || ''
        if (text.length > 20) {
          content += `Message ${i + 1}:\n${text}\n\n`
        }
      })
      
      content = content.trim()
      if (content.length < 50) {
        console.log('metldr: extracted content too short')
        return null
      }
      
      console.log('metldr: DOM extraction successful', { subject: subject.slice(0, 30), len: content.length })
      
      return {
        content,
        metadata: { subject, from }
      }
    } catch (err) {
      console.error('metldr: DOM extraction error:', err)
      return null
    }
  }
}
