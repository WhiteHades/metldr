// model selection for Ollama backend
// automatically detects available models and provides smart fallbacks

export class ModelRouter {
  constructor(ollamaClient) {
    this.client = ollamaClient;
    this.availableModels = [];
    this.defaultModel = 'llama3.2:3b';
  }

  // detect available models from Ollama
  async detectModels() {
    const { connected, models } = await this.client.checkConnection();
    
    if (connected && models.length > 0) {
      this.availableModels = models;
      // Set default to first available model if current default not found
      if (!this.hasModel(this.defaultModel)) {
        this.defaultModel = models[0];
      }
    }

    return this.availableModels;
  }

  // get best model for task with smart fallbacks
  getModel(taskType = 'general') {
    // Task-specific model preferences (Ollama models only)
    const preferences = {
      'email_summary': ['llama3.2:3b', 'qwen2.5:3b', 'llama3.2:1b'],
      'general': ['llama3.2:3b', 'qwen2.5:3b', 'llama3.1:8b'],
      'detailed': ['llama3.1:8b', 'llama3.2:3b', 'qwen2.5:7b']
    };

    const preferred = preferences[taskType] || preferences['general'];
    
    // find first available model from preferences
    for (const model of preferred) {
      const found = this.availableModels.find(m => m.includes(model));
      if (found) return found;
    }

    return this.availableModels[0] || this.defaultModel;
  }

  // check if model is available
  hasModel(modelName) {
    return this.availableModels.some(m => m.includes(modelName));
  }

  // get all available models
  getAvailableModels() {
    return this.availableModels;
  }

  // format model name 
  formatModelName(modelName) {
    if (!modelName) return 'No model selected';
    return modelName
      .split(':')[0]
      .split(/[._-]/)
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  }
}

