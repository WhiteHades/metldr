import { UIService } from './UIService'
import { WordPopup } from './WordPopup'
import { emailExtractor } from './EmailExtractor'
import { ArticleExtractor } from './ArticleExtractor'
import type { ThemeColors } from '../types'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type InboxSDK = any

export class ContentScriptBootstrap {
  static async init(sdk: InboxSDK | null = null): Promise<void> {
    const isGmail = window.location.hostname.includes('mail.google.com')

    UIService.init()
    await UIService.loadFromStorage()

    this.setupThemeListeners()
    this.setupMessageListeners()

    const popupManager = new WordPopup()
    document.addEventListener('mouseup', (e) => popupManager.handleTextSelection(e))

    if (isGmail && sdk) {
      emailExtractor.init(sdk, () => {
        // no callback, uses event handlers
      })
      console.log('metldr: gmail mode with inboxsdk')
    } else if (isGmail) {
      console.warn('metldr: gmail detected but no sdk available')
    } else {
      console.log('metldr: non-gmail mode, word popup only')
    }
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
}
