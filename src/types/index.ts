export interface Theme {
  primary: string
  secondary: string
  accent: string
  bg: string
  bgSecondary: string
  text: string
  textMuted: string
  border: string
  borderSubtle: string
  shadow: string
}

export type ThemeName = 'light' | 'cyberpunk' | 'catppuccin' | 'gruvbox'
export type ThemeListener = (themeName: string, theme: Theme) => void

export interface ThemeColors {
  [key: string]: string
}

export interface IntentStyle {
  bg: string
  text: string
}

export interface AmountFact {
  label: string
  value: string
  currency: string | null
}

export interface IdFact {
  label: string
  value: string
}

export interface DateFact {
  label: string
  when: string
}

export interface ContactFact {
  type: string
  value: string
}

export interface LinkFact {
  label: string
  url: string
}

export interface ExtractedFacts {
  amounts: AmountFact[]
  ids: IdFact[]
  dates: DateFact[]
  contacts: ContactFact[]
  links: LinkFact[]
  people: string[]
  locations: string[]
  action_items: string[]
}

export interface EmailMetadata {
  from?: string | null
  sender?: string | null
  fromEmail?: string | null
  senderEmail?: string | null
  replyTo?: string | null
  'reply-to'?: string | null
  to?: string | null
  toList?: string[]
  toRecipients?: string[]
  cc?: string | null
  ccList?: string[]
  ccRecipients?: string[]
  bcc?: string | null
  bccList?: string[]
  bccRecipients?: string[]
  date?: string | null
  subject?: string
  mailedBy?: string | null
  signedBy?: string | null
  participants?: string[]
  emailCount?: number
}

export interface ReplySuggestion {
  id: string
  tone: string
  length: string
  body: string
  label: string
}

export interface EmailSummary {
  summary: string
  action_items: string[]
  dates: string[]
  key_facts: {
    booking_reference: string | null
    amount: string | null
    sender_org: string | null
  }
  intent: string | null
  tags?: string[]
  domain?: string | null
  reasoning: string | null
  urgency: string
  time_ms?: number
  cached?: boolean
  model?: string
  ner_tags?: { label: string; score: number }[]
  entities?: { word: string; entity: string; score: number }[]
  confidence?: 'high' | 'medium' | 'low'
}

export interface EmailSession {
  emailId: string
  summary: EmailSummary | null
  replySuggestions: ReplySuggestion[] | null
  chatMessages: AppChatMessage[]
  metadata: EmailMetadata | null
  timestamp: number
  ttl: number
}

export interface ParsedLLMSummary {
  summary?: string
  action_items?: string[]
  intent?: string
  tags?: string[]
  domain?: string
  reasoning?: string
  urgency?: string
  key_details?: {
    main_date?: string
    booking_reference?: string
    amount?: string
    sender_org?: string
  }
}

export interface ParsedReply {
  tone?: string
  length?: string
  body?: string
  label?: string
}

export interface ParsedReplies {
  replies?: ParsedReply[]
}

// page
export interface ExtractedData {
  title: string
  url?: string
  content: string
  author?: string
  publishDate?: string
  publication?: string
  wordCount?: number
  readTime?: string
  extractionTime?: number
  contentType?: string
  pageSignals?: PageSignals
  skip?: boolean
  reason?: string
}

export interface SummaryTiming {
  extraction?: number
  llm?: number
  total: number
  cached?: boolean
  model?: string
  provider?: 'chrome-ai' | 'ollama'
}

export interface PageSummary {
  title: string
  author?: string
  publishDate?: string
  publication?: string
  bullets: string[]
  readTime?: string
  content?: string
  fullContent?: string
  wordCount?: number
  timestamp?: number
  timing?: SummaryTiming
}

export interface PageContext {
  fullContent?: string
}

export interface PageInfo {
  type: string
  confidence: number
  metadata: {
    url: string
    hostname: string
  }
}

export interface ExtractedContent {
  content: string
  sections: string[]
  headers: string[]
}

export interface PreSummarizeData {
  priority: string
  url: string
  pageType: string
  metadata: { url: string; hostname: string }
  content: string
  sections: string[]
}

export type PreSummarizeCallback = (data: PreSummarizeData) => void

export interface PageSignals {
  isSPA?: boolean
  isDashboard?: boolean
  isSearch?: boolean
  isCart?: boolean
  isFeed?: boolean
  isSocialFeed?: boolean
  canvasHeavy?: boolean
  buttonToParagraphRatio?: number
  linkDensity?: number
  hasArticleTag?: boolean
  h1Count?: number
  textDensity?: number
  hasMain?: boolean
}

export interface ClassifiedPage {
  url?: string
  contentType?: string
  pageSignals?: PageSignals
  wordCount?: number
  skip?: boolean
  reason?: string
}

export interface ClassifyResult {
  action: 'skip' | 'auto' | 'prompt' | 'wait'
  reason: string
}

export interface SummaryResult {
  success: boolean
  summary?: unknown
  error?: string
  skip?: boolean
  prompt?: boolean
  reason?: string
}

// word/dictionary
export interface Definition {
  definition: string
  partOfSpeech: string
  example: string | null
  synonyms: string[]
}

export interface WordLookupResult {
  definitions: Definition[]
  synonyms: string[]
  language: string
  source?: string
}

export interface LookupContext {
  languages?: string[]
  fullSentence?: string
  contextBefore?: string
  contextAfter?: string
}

export interface ApiMeaning {
  partOfSpeech: string
  definitions: Array<{
    definition: string
    example?: string
    synonyms?: string[]
  }>
}

export interface ApiEntry {
  word: string
  meanings: ApiMeaning[]
}

export interface LLMParsedResult {
  definition?: string
  partOfSpeech?: string
  example?: string
  synonyms?: string[]
}

export interface DictionaryEntry {
  definitions: Array<{
    definition: string
    partOfSpeech: string
    example: string | null
    synonyms: string[]
  }>
  synonyms: string[]
  language: string
  source?: string
}

export interface DictEntry {
  word: string
  pos?: string
  definition: string
}

export interface DictMetaEntry {
  key: string
  downloaded?: boolean
  timestamp?: number
}

export interface DictLookupResult {
  word: string
  pos: string
  definition: string
  source?: string
}

export interface ContextData {
  contextBefore?: string
  contextAfter?: string
  fullSentence?: string
}

export interface LookupResponse {
  success?: boolean
  error?: string
  result?: {
    word?: string
    definitions?: Definition[]
    synonyms?: string[]
    source?: string
    translation?: string
    sourceLang?: string
    targetLang?: string
  }
}

// ollama/llm
export interface OllamaModel {
  name: string
}

export interface OllamaTagsResponse {
  models?: OllamaModel[]
}

export interface ChatMessage {
  role: string
  content: string
  timing?: { total: number; model?: string }
}

export interface CompleteOptions {
  temperature?: number
  maxTokens?: number
}

export interface CompleteResult {
  ok: boolean
  content?: string
  error?: string
}

export interface ChatResult {
  ok: boolean
  content?: string
  error?: string
  timing?: { total: number; model: string }
}

export type TaskType = 'word_lookup' | 'page_summary' | 'email_summary'

// storage
export interface SupportedLanguage {
  code: string
  name: string
  url: string
}

export interface CacheEntry {
  key: string
  value: unknown
  timestamp: number
  ttl: number
}

export interface EmailSummaryEntry {
  emailId: string
  summary: unknown
  timestamp: number
  [key: string]: unknown
}

export interface WordDefinitionEntry {
  word: string
  definition: unknown
}

export interface DownloadProgress {
  letter: string
  progress: number
  entriesProcessed: number
}

export interface ResumeState {
  nextIndex: number
  entriesProcessed: number
}

// background messages
export interface EmailSummaryMessage {
  type: 'SUMMARIZE_EMAIL'
  emailContent: string
  emailId?: string
  metadata?: Record<string, unknown>
  forceRegenerate?: boolean
}

export interface GetReplySuggestionsMessage {
  type: 'GET_REPLY_SUGGESTIONS'
  emailId: string
}

export interface GenerateReplySuggestionsMessage {
  type: 'GENERATE_REPLY_SUGGESTIONS'
  emailId: string
  forceRegenerate?: boolean
}

export interface ExtractAndSummarizeMessage {
  type: 'EXTRACT_AND_SUMMARIZE'
  tabId: number
  force?: boolean
  trigger?: string
}

export interface WordLookupMessage {
  type: 'WORD_LOOKUP'
  word: string
  context?: {
    fullSentence?: string
    languages?: string[]
  }
}

export interface ChatMessageRequest {
  type: 'CHAT_MESSAGE'
  model?: string
  messages: ChatMessage[]
  pageContext?: PageContext
}

export interface CheckHealthMessage {
  type: 'CHECK_OLLAMA_HEALTH'
}

export interface GetEmailCacheMessage {
  type: 'GET_EMAIL_CACHE'
  emailId: string
}

export interface SetEmailCacheMessage {
  type: 'SET_EMAIL_CACHE'
  emailId: string
  summary: EmailSummary
}

export type BackgroundMessage =
  | EmailSummaryMessage
  | GetReplySuggestionsMessage
  | GenerateReplySuggestionsMessage
  | ExtractAndSummarizeMessage
  | WordLookupMessage
  | ChatMessageRequest
  | CheckHealthMessage
  | GetEmailCacheMessage
  | SetEmailCacheMessage
  | { type: 'EXTRACT_ONLY'; tabId: number }
  | { type: 'RAG_INDEX'; entry: VectorEntry }
  | { type: 'RAG_INDEX_CHUNKS'; text: string; metadata: Record<string, unknown> }
  | { type: 'RAG_SEARCH'; query: string; limit?: number }
  | { type: 'RAG_HAS_INDEXED_CONTENT'; sourceUrl: string }
  | { type: 'RAG_SEARCH_WITH_CONTEXT'; query: string; limit?: number; sourceUrl?: string }
  | { type: 'RAG_IS_INDEXING'; sourceId: string }
  | { type: 'GLOBAL_CHAT'; messages: ChatMessage[] }
  | { type: 'PDF_SUMMARIZE'; url: string }
  | { type: 'PDF_EXTRACT_TEXT'; url: string }
  | { type: 'OPEN_SIDE_PANEL'; focus?: string }
  | { type: 'TOGGLE_SIDE_PANEL'; focus?: string }
  | { type: 'PDF_PROCESS_ARRAYBUFFER'; data: number[]; filename: string; action: 'summarize' | 'copy'; sourceUrl?: string }
  | { type: 'GET_PAGE_CACHE'; url: string }

export type ResponseCallback = (response: unknown) => void

// summary prefs
export interface SummaryPrefsConfig {
  mode: string
  allowlist: string[]
  denylist: string[]
  minAutoWords: number
  minPromptWords: number
}

export interface SummaryPrefsType {
  mode: string
  allowlist: string[]
  denylist: string[]
  minAutoWords: number
  minPromptWords: number
}

// ui summary display
export interface Summary {
  summary?: string
  action_items?: string[]
  dates?: string[]
  confidence?: 'high' | 'medium' | 'low'
  model?: string
  intent?: string
  time_ms?: number
  cached?: boolean
}

// inboxsdk types
export interface InboxSDKCompose {
  registerComposeViewHandler: (handler: (composeView: ComposeView) => void) => void
}

export interface InboxSDK {
  Compose: InboxSDKCompose
}

export interface ComposeView {
  isInlineReplyForm?: () => boolean
  getThreadView?: () => ThreadView | null
  getThreadID?: () => string
  getElement?: () => HTMLElement | null
  getToRecipients: () => Contact[]
  getCcRecipients: () => Contact[]
  getBccRecipients: () => Contact[]
  getSubject: () => string
  getTextContent: () => string
  getHTMLContent: () => string
  insertHTMLIntoBodyAtCursor: (html: string) => void
  insertTextIntoBodyAtCursor: (text: string) => void
  setBodyHTML: (html: string) => void
  addButton: (options: { title: string; iconUrl: string; orderHint: number; onClick: (event: Event) => void }) => void
  on: (event: string, callback: () => void) => void
}

export interface ThreadView {
  getThreadIDAsync?: () => Promise<string>
  getSubject: () => string
  getMessageViewsAll: () => MessageView[]
  getElement?: () => HTMLElement | null
}

export interface MessageView {
  getBodyElement: () => HTMLElement | null
  getSender: () => Contact | null
  getRecipientEmailAddresses: () => string[]
  getRecipientsFull: () => Promise<Contact[]>
  getReplyTo?: () => Contact | Contact[] | null
  getSMTPHeaders?: () => Record<string, string> | null
  getDateString?: () => string | null
  isLoaded?: () => boolean
}

export interface Contact {
  name?: string
  fullName?: string
  displayName?: string
  emailAddress?: string
  address?: string
  email?: string
}

// email extractor
export type EmailProcessCallback = () => void

export interface SummaryResponse {
  summary?: unknown
  action_items?: string[]
  dates?: string[]
  key_facts?: {
    booking_reference?: string
    amount?: string
    sender_org?: string
  }
  intent?: string
  reasoning?: string
  urgency?: string
  time_ms?: number
  cached?: boolean
  model?: string
  error?: string
  needsOllama?: boolean
}

export interface HealthResponse {
  healthy?: boolean
  models?: string[]
  error?: string
  success?: boolean
  connected?: boolean
}

// app state
export interface AppPageSummary {
  title: string
  author?: string
  publishDate?: string
  publication?: string
  bullets: string[]
  readTime?: string
  content?: string
  fullContent?: string
  wordCount?: number
  timestamp?: number
  timing?: SummaryTiming
}

export interface AppChatMessage {
  role: 'user' | 'assistant'
  content: string
  timing?: { 
    total: number
    model?: string
    rag?: number      // total rag time
    embed?: number    // query embedding time
    search?: number   // voy search time
    llm?: number      // llm generation time
  }
}

export interface SummaryPromptData {
  url?: string
  title?: string
  wordCount?: number
  reason?: string
}

export interface DropdownPos {
  top: number
  left: number
  width: number
}

export interface DownloadProgressItem {
  language?: string
  letter: string
  progress: number
  entriesProcessed?: number
  entries?: number
}

export interface OllamaHealthResponse {
  healthy?: boolean
  models?: string[]
  error?: string
  success?: boolean
  connected?: boolean
}

export interface AppSummaryResponse {
  success: boolean
  summary?: AppPageSummary
  error?: string
  skip?: boolean
  prompt?: boolean
  reason?: string
}

export interface AppChatResponse {
  ok: boolean
  content?: string
  timing?: { total: number; model: string }
  error?: string
}

// history
export interface HistoryItem {
  emailId: string
  timestamp: number
  summary?: {
    summary?: string
    time_ms?: number
    cached?: boolean
  }
}

// reply panel
export interface FetchResponse {
  success: boolean
  error?: string
  replies?: ReplySuggestion[]
  suggestions?: ReplySuggestion[]
  contextInvalidated?: boolean
}

// rag & pdf
export interface VectorEntry {
  id: string
  type: 'email' | 'page' | 'pdf' | 'article'
  content: string
  metadata: Record<string, unknown>
  timestamp: number
}

export interface SearchResult {
  entry: VectorEntry
  score: number
  matchType?: 'semantic' | 'keyword' | 'hybrid'
}

export interface PdfChunk {
  text: string
  pageStart: number
  pageEnd: number
  tokenCountEstimate: number
}
