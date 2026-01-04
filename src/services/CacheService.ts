import { databaseService, DB_CONFIGS } from './DatabaseService'
import { logger } from './LoggerService'
import type { 
  AppPageSummary, 
  AppChatMessage, 
  EmailSession, 
  EmailSummary, 
  ReplySuggestion, 
  EmailMetadata 
} from '@/types'

const log = logger.createScoped('CacheService')
const STORE_EMAIL_SESSIONS = 'email_sessions'
const STORE_PAGE_CACHE = 'page_cache'
const STORE_TAB_SESSIONS = 'tab_sessions'

const DEFAULT_TTL_MS = 7 * 24 * 60 * 60 * 1000 // 7 days

interface CacheEntry<T> {
  timestamp: number
  ttl: number
  data: T
}

interface PageSummaryEntry extends CacheEntry<unknown> {
  url: string
  summary: unknown
}

export interface TabSessionEntry {
  url: string
  timestamp: number
  chatMessages: AppChatMessage[]
  pageSummary: AppPageSummary | null
  summaryCollapsed: boolean
}

export class CacheService {
  // ttl disabled - keep all data forever (storage is minimal)
  private isExpired(_entry: { timestamp?: number; ttl?: number }): boolean {
    return false
  }

  async getEmailSession(emailId: string): Promise<EmailSession | null> {
    try {
      const result = await databaseService.get<EmailSession>(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS, emailId)
      if (!result) return null
      if (this.isExpired(result)) {
        this.deleteEmailSession(emailId).catch(() => {})
        return null
      }
      return result
    } catch (err) {
      log.error('getEmailSession failed', (err as Error).message)
      return null
    }
  }

  async updateEmailSession(emailId: string, updates: Partial<Omit<EmailSession, 'emailId'>>): Promise<void> {
    try {
      const existing = await databaseService.get<EmailSession>(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS, emailId)
      const now = Date.now()
      
      const session: EmailSession = {
        emailId,
        summary: existing?.summary ?? null,
        replySuggestions: existing?.replySuggestions ?? null,
        chatMessages: existing?.chatMessages ?? [],
        metadata: existing?.metadata ?? null,
        timestamp: now,
        ttl: updates.ttl ?? existing?.ttl ?? DEFAULT_TTL_MS,
        ...updates
      }
      
      await databaseService.put(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS, session)
      
      if (updates.summary && !existing?.summary) {
        chrome.runtime.sendMessage({ type: 'SUMMARY_ADDED', emailId }).catch(() => {})
      }
    } catch (err) {
      log.error('updateEmailSession failed', (err as Error).message)
    }
  }

  async deleteEmailSession(emailId: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS, emailId)
    } catch (err) {
      log.error('deleteEmailSession failed', (err as Error).message)
    }
  }

  async getEmailSummary(emailId: string): Promise<EmailSummary | null> {
    const session = await this.getEmailSession(emailId)
    return session?.summary ?? null
  }

  async setEmailSummary(emailId: string, summary: EmailSummary, metadata?: EmailMetadata | null): Promise<void> {
    await this.updateEmailSession(emailId, { summary, ...(metadata !== undefined && { metadata }) })
  }

  async deleteEmailSummary(emailId: string): Promise<void> {
    await this.updateEmailSession(emailId, { summary: null })
  }

  async getReplySuggestions(emailId: string): Promise<ReplySuggestion[] | null> {
    const session = await this.getEmailSession(emailId)
    return session?.replySuggestions ?? null
  }

  async setReplySuggestions(emailId: string, suggestions: ReplySuggestion[]): Promise<void> {
    await this.updateEmailSession(emailId, { replySuggestions: suggestions })
  }

  async deleteReplySuggestions(emailId: string): Promise<void> {
    await this.updateEmailSession(emailId, { replySuggestions: null })
  }

  async getEmailChat(emailId: string): Promise<AppChatMessage[]> {
    const session = await this.getEmailSession(emailId)
    return session?.chatMessages ?? []
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
      await databaseService.put(DB_CONFIGS.cache, STORE_PAGE_CACHE, {
        url,
        summary,
        timestamp: Date.now(),
        ttl: ttlSeconds * 1000
      })
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
      const result = await databaseService.get<TabSessionEntry>(DB_CONFIGS.cache, STORE_TAB_SESSIONS, url)
      return result || null
    } catch (err) {
      log.error('getTabSession failed', (err as Error).message)
      return null
    }
  }

  async setTabSession(
    url: string, 
    chatMessages: AppChatMessage[], 
    pageSummary: AppPageSummary | null, 
    summaryCollapsed: boolean
  ): Promise<void> {
    try {
      const plainMessages = JSON.parse(JSON.stringify(chatMessages))
      const plainSummary = pageSummary ? JSON.parse(JSON.stringify(pageSummary)) : null
      
      await databaseService.put(DB_CONFIGS.cache, STORE_TAB_SESSIONS, {
        url,
        chatMessages: plainMessages,
        pageSummary: plainSummary,
        summaryCollapsed,
        timestamp: Date.now()
      })
    } catch (err) {
      log.error('setTabSession failed', (err as Error).message)
    }
  }

  async deleteTabSession(url: string): Promise<void> {
    try {
      await databaseService.delete(DB_CONFIGS.cache, STORE_TAB_SESSIONS, url)
    } catch (err) {
      log.error('deleteTabSession failed', (err as Error).message)
    }
  }

  async getAllTabSessions(): Promise<TabSessionEntry[]> {
    try {
      return await databaseService.getAll<TabSessionEntry>(DB_CONFIGS.cache, STORE_TAB_SESSIONS)
    } catch (err) {
      log.error('getAllTabSessions failed', (err as Error).message)
      return []
    }
  }

  async clearAll(): Promise<void> {
    try {
      await Promise.all([
        databaseService.clear(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS),
        databaseService.clear(DB_CONFIGS.cache, STORE_PAGE_CACHE),
        databaseService.clear(DB_CONFIGS.cache, STORE_TAB_SESSIONS)
      ])
    } catch (err) {
      log.error('clearAll failed', (err as Error).message)
    }
  }

  async getAllEmailSessions(): Promise<EmailSession[]> {
    try {
      return await databaseService.getAll<EmailSession>(DB_CONFIGS.cache, STORE_EMAIL_SESSIONS)
    } catch (err) {
      log.error('getAllEmailSessions failed', (err as Error).message)
      return []
    }
  }
}

export const cacheService = new CacheService()
