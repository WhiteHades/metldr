export const OLLAMA_BASE_URL = 'http://127.0.0.1:11434';

export const TIMEOUTS = {
  HEALTH_CHECK: 2000,  // 2s for ollama health check
  INFERENCE: 30000,    // 30s for LLM inference
  MODEL_FALLBACK: 30000 // 30s per model before trying next
};

export const MODEL_PREFERENCES = {
  email_summary: ['llama3.2:3b', 'qwen2.5:3b', 'llama3.2:1b'],
  chat: ['llama3.2:3b', 'qwen2.5:3b', 'llama3.1:8b'],
  word_definition: ['llama3.2:1b', 'qwen2.5:1.5b'],
  general: ['llama3.2:3b', 'qwen2.5:3b', 'llama3.1:8b']
};

export const CACHE_TTL = {
  EMAIL_SUMMARY: 7 * 24 * 60 * 60 * 1000,  // 7 days
  WORD_DEFINITION: 30 * 24 * 60 * 60 * 1000, // 30 days
  MODEL_LIST: 5 * 60 * 1000  // 5 minutes
};

export const DB_CONFIG = {
  NAME: 'metldr_cache',
  VERSION: 1,
  STORES: {
    SUMMARIES: 'email_summaries',
    OLLAMA_CACHE: 'ollama_cache',
    DEFINITIONS: 'word_definitions',
    HISTORY: 'summary_history'
  }
};

export const MESSAGE_TYPES = {
  SUMMARIZE_EMAIL: 'SUMMARISE_EMAIL',
  GET_HISTORY: 'GET_HISTORY',
  CLEAR_CACHE: 'CLEAR_CACHE',
  GET_STATS: 'GET_STATS',
  UPDATE_SETTINGS: 'UPDATE_SETTINGS'
};
