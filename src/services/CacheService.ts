import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'
import {
  EMAIL_STORE_NAMES,
  EMAIL_STORES,
  SESSION_STORE_NAMES,
  TAB_STORE_NAMES,
  TAB_STORES,
  request,
  writeEmailSession,
  writeEmailUpdate,
  writeTabSession,
  type CachedEmailSession,
  type EmailHeader,
  type EmailPayload,
  type TabHeader,
  type TabPayload,
  type TabSummaryPayload,
  type TabSessionEntry
} from './cacheRecords'
import type { AppPageSummary, AppChatMessage, EmailSession, EmailSummary, ReplySuggestion, EmailMetadata } from '@/types'

const log = logger.createScoped('CacheService')
const STORE_PAGE_CACHE = 'page_cache'

interface CacheEntry<T> {
  timestamp: number
  ttl: number
  data: T
}

interface PageSummaryEntry extends CacheEntry<unknown> {
  url: string
  summary: unknown
}

export class CacheService {
  // ttl disabled - keep all data forever (storage is minimal)
  private isExpired(_entry: { timestamp?: number; ttl?: number }): boolean {
    return false
  }

  async updateEmailSession(emailId: string, updates: Partial<Omit<EmailSession, 'emailId'>> & { emailCount?: number }): Promise<void> {
    try {
      const notify = await databaseService.transaction(DB_CONFIGS.cache, EMAIL_STORE_NAMES, 'readwrite',
        tx => writeEmailUpdate(tx, emailId, updates))
      if (notify) chrome.runtime.sendMessage({ type: 'SUMMARY_ADDED', emailId }).catch(() => {})
    } catch (err) {
      log.error('updateEmailSession failed', (err as Error).message)
    }
  }

  async deleteEmailSession(emailId: string): Promise<void> {
    try {
      await databaseService.transaction(DB_CONFIGS.cache, EMAIL_STORE_NAMES, 'readwrite', tx => {
        for (const name of EMAIL_STORE_NAMES) tx.objectStore(name).delete(emailId)
      })
    } catch (err) {
      log.error('deleteEmailSession failed', (err as Error).message)
    }
  }

  async getEmailSummary(emailId: string): Promise<EmailSummary | null> {
    try {
      const payload = await databaseService.get<EmailPayload<EmailSummary | null>>(DB_CONFIGS.cache, EMAIL_STORES.summary, emailId)
      return payload?.value ?? null
    } catch (err) {
      log.error('getEmailSummary failed', (err as Error).message)
      return null
    }
  }

  async getEmailCache(emailId: string): Promise<{ summary: EmailSummary | null; emailCount: number }> {
    try {
      return await databaseService.transaction(DB_CONFIGS.cache, [EMAIL_STORES.header, EMAIL_STORES.summary], 'readonly', async tx => {
        const [header, payload] = await Promise.all([
          request<EmailHeader | undefined>(tx.objectStore(EMAIL_STORES.header).get(emailId)),
          request<EmailPayload<EmailSummary | null> | undefined>(tx.objectStore(EMAIL_STORES.summary).get(emailId))
        ])
        return { summary: payload?.value ?? null, emailCount: header?.emailCount || 0 }
      })
    } catch (err) {
      log.error('getEmailCache failed', (err as Error).message)
      return { summary: null, emailCount: 0 }
    }
  }

  async setEmailSummary(emailId: string, summary: EmailSummary, metadata?: EmailMetadata | null): Promise<void> {
    await this.updateEmailSession(emailId, { summary, ...(metadata !== undefined && { metadata }) })
  }

  async deleteEmailSummary(emailId: string): Promise<void> {
    await this.updateEmailSession(emailId, { summary: null })
  }

  async getReplySuggestions(emailId: string): Promise<ReplySuggestion[] | null> {
    try {
      const payload = await databaseService.get<EmailPayload<ReplySuggestion[] | null>>(DB_CONFIGS.cache, EMAIL_STORES.replies, emailId)
      return payload?.value ?? null
    } catch (err) {
      log.error('getReplySuggestions failed', (err as Error).message)
      return null
    }
  }

  async setReplySuggestions(emailId: string, suggestions: ReplySuggestion[]): Promise<void> {
    await this.updateEmailSession(emailId, { replySuggestions: suggestions })
  }

  async deleteReplySuggestions(emailId: string): Promise<void> {
    await this.updateEmailSession(emailId, { replySuggestions: null })
  }

  async getEmailChat(emailId: string): Promise<AppChatMessage[]> {
    try {
      const payload = await databaseService.get<EmailPayload<AppChatMessage[]>>(DB_CONFIGS.cache, EMAIL_STORES.chat, emailId)
      return payload?.value ?? []
    } catch (err) {
      log.error('getEmailChat failed', (err as Error).message)
      return []
    }
  }

  async setEmailChat(emailId: string, messages: AppChatMessage[]): Promise<void> {
    const plainMessages = JSON.parse(JSON.stringify(messages)) as AppChatMessage[]
    await this.updateEmailSession(emailId, { chatMessages: plainMessages })
  }

  async getPageSummary(url: string): Promise<unknown> {
    try {
      const result = await databaseService.get<PageSummaryEntry>(DB_CONFIGS.cache, STORE_PAGE_CACHE, url)
      if (!result) return null
      if (this.isExpired(result)) {
        this.deletePageSummary(url).catch(() => {})
        return null
      }
      return result.summary
    } catch (err) {
      log.error('getPageSummary failed', (err as Error).message)
      return null
    }
  }

  async setPageSummary(url: string, summary: unknown, ttlSeconds = 3600): Promise<void> {
    try {
      await databaseService.put(DB_CONFIGS.cache, STORE_PAGE_CACHE, { url, summary, timestamp: Date.now(), ttl: ttlSeconds * 1000 })
    } catch (err) {
      log.error('setPageSummary failed', (err as Error).message)
    }
  }

  async deletePageSummary(url: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_PAGE_CACHE, url)
    } catch (err) {
      log.error('deletePageSummary failed', (err as Error).message)
    }
  }

  async getTabSession(url: string): Promise<TabSessionEntry | null> {
    try {
      return await databaseService.transaction(DB_CONFIGS.cache, TAB_STORE_NAMES, 'readonly', async tx => {
        const [header, summary, chat] = await Promise.all([
          request<TabHeader | undefined>(tx.objectStore(TAB_STORES.header).get(url)),
          request<TabSummaryPayload | undefined>(tx.objectStore(TAB_STORES.summary).get(url)),
          request<TabPayload<AppChatMessage[]> | undefined>(tx.objectStore(TAB_STORES.chat).get(url))
        ])
        if (!header) return null
        return {
          ...summary?.extra,
          url,
          timestamp: header.timestamp,
          pageSummary: summary?.value ?? null,
          chatMessages: chat?.value ?? [],
          summaryCollapsed: header.summaryCollapsed
        }
      })
    } catch (err) {
      log.error('getTabSession failed', (err as Error).message)
      return null
    }
  }

  async setTabSession(url: string, chatMessages: AppChatMessage[], pageSummary: AppPageSummary | null, summaryCollapsed: boolean): Promise<void> {
    try {
      const plainMessages = JSON.parse(JSON.stringify(chatMessages)) as AppChatMessage[]
      const plainSummary = pageSummary ? JSON.parse(JSON.stringify(pageSummary)) as AppPageSummary : null
      await databaseService.transaction(DB_CONFIGS.cache, TAB_STORE_NAMES, 'readwrite', tx => {
        writeTabSession(tx, { url, chatMessages: plainMessages, pageSummary: plainSummary, summaryCollapsed, timestamp: Date.now() })
      })
    } catch (err) {
      log.error('setTabSession failed', (err as Error).message)
    }
  }

  async deleteTabSession(url: string): Promise<void> {
    try {
      await databaseService.transaction(DB_CONFIGS.cache, TAB_STORE_NAMES, 'readwrite', tx => {
        for (const name of TAB_STORE_NAMES) tx.objectStore(name).delete(url)
      })
    } catch (err) {
      log.error('deleteTabSession failed', (err as Error).message)
    }
  }

  async clearSessions(): Promise<void> {
    try {
      await databaseService.transaction(DB_CONFIGS.cache, SESSION_STORE_NAMES, 'readwrite', tx => {
        for (const name of SESSION_STORE_NAMES) tx.objectStore(name).clear()
      })
    } catch (err) {
      log.error('clearSessions failed', (err as Error).message)
    }
  }

  async clearAll(): Promise<void> {
    try {
      await databaseService.transaction(DB_CONFIGS.cache, [...SESSION_STORE_NAMES, STORE_PAGE_CACHE], 'readwrite', tx => {
        for (const name of [...SESSION_STORE_NAMES, STORE_PAGE_CACHE]) tx.objectStore(name).clear()
      })
    } catch (err) {
      log.error('clearAll failed', (err as Error).message)
    }
  }

  async putEmailSessions(sessions: CachedEmailSession[]): Promise<void> {
    for (let i = 0; i < sessions.length; i += 500) {
      const batch = sessions.slice(i, i + 500)
      await databaseService.transaction(DB_CONFIGS.cache, EMAIL_STORE_NAMES, 'readwrite', tx => {
        for (const session of batch) writeEmailSession(tx, session)
      })
    }
  }

  async putTabSessions(sessions: TabSessionEntry[]): Promise<void> {
    for (let i = 0; i < sessions.length; i += 500) {
      const batch = sessions.slice(i, i + 500)
      await databaseService.transaction(DB_CONFIGS.cache, TAB_STORE_NAMES, 'readwrite', tx => {
        for (const session of batch) writeTabSession(tx, session)
      })
    }
  }
}

export { type TabSessionEntry } from './cacheRecords'
export const cacheService = new CacheService()
