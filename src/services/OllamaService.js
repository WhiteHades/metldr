import { stripThinking } from '../lib/textUtils.js';

export class OllamaService {
  static BASE_URL = 'http://127.0.0.1:11434';
  static TIMEOUT_HEALTH = 3000;
  static TIMEOUT_CHAT = 90000;
  static TIMEOUT_CHAT_LONG = 180000;

  static PRIORITY = {
    word_lookup: ['llama3.2:1b', 'qwen2.5:1.5b', 'llama3.2:3b'],
    page_summary: ['llama3.2:3b', 'llama3.2:1b', 'qwen2.5:1.5b'],
    email_summary: ['llama3.2:3b', 'llama3.2:1b', 'qwen2.5:3b']
  };

  static async checkAvailable() {
    try {
      const res = await fetch(`${this.BASE_URL}/api/tags`, {
        signal: AbortSignal.timeout(this.TIMEOUT_HEALTH)
      });

      if (!res.ok) {
        console.warn('[OllamaService.checkAvailable] bad status:', res.status);
        return { available: false, models: [], reason: `status ${res.status}` };
      }

      const data = await res.json();
      const models = data.models?.map(m => m.name) || [];

      console.log('[OllamaService.checkAvailable] connected, models:', models.length);
      return { available: true, models };
    } catch (err) {
      const isCors = err.message === 'Failed to fetch' || err.name === 'TypeError';
      const reason = isCors ? 'cors_blocked' : err.message;
      console.error('[OllamaService.checkAvailable]', err.message, isCors ? '(likely CORS - restart ollama with OLLAMA_ORIGINS)' : '');
      return { available: false, models: [], reason };
    }
  }

  static async complete(model, messages, options = {}) {
    try {
      const body = {
        model,
        messages,
        stream: false,
        options: {
          temperature: options.temperature ?? 0,
          top_p: options.top_p ?? 0.9
        }
      };

      if (options.format) body.format = options.format;

      const timeout = options.longContext ? this.TIMEOUT_CHAT_LONG : this.TIMEOUT_CHAT;

      const res = await fetch(`${this.BASE_URL}/api/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(timeout)
      });

      if (!res.ok) {
        return {
          ok: false,
          error: `ollama returned ${res.status}`
        };
      }

      const data = await res.json();
      const rawContent = data?.message?.content || data?.response;
      const content = stripThinking(rawContent);

      return { ok: true, content };
    } catch (err) {
      if (err.name === 'AbortError') {
        return { ok: false, error: 'timeout' };
      }
      console.error('[OllamaService.complete]', err.message);
      return { ok: false, error: err.message };
    }
  }

  static async getUserSelected() {
    try {
      const result = await chrome.storage.local.get('selectedModel');
      return result.selectedModel || null;
    } catch (err) {
      console.error('[OllamaService.getUserSelected]', err.message);
      return null;
    }
  }

  static async listAvailable() {
    const { available, models } = await this.checkAvailable();
    return available ? models : [];
  }

  static async selectBest(taskType = 'email_summary') {
    const userModel = await this.getUserSelected();
    if (userModel) return userModel;

    const available = await this.listAvailable();
    if (!available.length) return null;

    const priority = this.PRIORITY[taskType] || [];
    for (const hint of priority) {
      const found = available.find(m => m.includes(hint));
      if (found) return found;
    }

    return available[0];
  }

  static async tryWithFallback(taskFn, taskType = 'email_summary') {
    const userModel = await this.getUserSelected();
    const priority = userModel ? [userModel] : (this.PRIORITY[taskType] || []);

    const available = await this.listAvailable();
    if (!available.length) throw new Error('no models available');

    const modelList = [];
    const seen = new Set();

    for (const hint of priority) {
      const found = available.find(m => m.includes(hint));
      if (found && !seen.has(found)) {
        modelList.push(found);
        seen.add(found);
      }
    }

    for (const model of available) {
      if (!seen.has(model)) {
        modelList.push(model);
        seen.add(model);
      }
    }

    for (const model of modelList) {
      try {
        return await taskFn(model);
      } catch (err) {
        console.log(`[OllamaService] model ${model} failed:`, err.message);
        continue;
      }
    }

    throw new Error(`all ${modelList.length} models failed`);
  }
}
