// Ollama client for LLM communication
// Supports streaming and structured JSON output with Ollama's format parameter
// All structured outputs use temperature=0 for deterministic, consistent results

export class OllamaClient {
  constructor(baseUrl = 'http://127.0.0.1:11434') {
    this.baseUrl = baseUrl;
    this.timeout = 30000; // 30s timeout for inference
  }

  // check if ollama is running and get available models
  async checkConnection() {
    try {
      const response = await fetch(`${this.baseUrl}/api/tags`, {
        signal: AbortSignal.timeout(2000)
      });
      
      if (!response.ok) {
        return { connected: false, models: [] };
      }

      const data = await response.json();
      const models = data.models?.map(m => m.name) || [];
      
      return { connected: true, models };
    } catch (error) {
      return { connected: false, models: [] };
    }
  }

  // generate completion with streaming
  async generate(prompt, options = {}) {
    const {
      model = 'llama3.2:3b',
      system = null,
      temperature = 0.3,
      onToken = null, // callback for streaming tokens
      onError = null
    } = options;

    const body = {
      model,
      prompt,
      stream: true,
      options: { temperature }
    };

    if (system) {
      body.system = system;
    }

    try {
      const response = await fetch(`${this.baseUrl}/api/generate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(this.timeout)
      });

      if (!response.ok) {
        throw new Error(`Ollama API error: ${response.status}`);
      }

      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      let fullText = '';

      while (true) {
        const { done, value } = await reader.read();

        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop(); // keep incomplete line in buffer

        for (const line of lines) {
          if (!line.trim()) continue;

          try {
            const json = JSON.parse(line);
            
            if (json.response) {
              fullText += json.response;
              if (onToken) onToken(json.response);
            }

            if (json.done) {
              return fullText;
            }
          } catch (e) {
            // incomplete json chunk, continue
          }
        }
      }

      return fullText;
    } catch (error) {
      if (onError) onError(error);
      throw error;
    }
  }

  // structured completion with JSON schema using Ollama's format parameter
  // follows Ollama best practices: temperature=0, proper schema format
  async generateStructured(prompt, schema, options = {}) {
    const {
      model = 'llama3.2:3b',
      system = null,
      temperature = 0  // Use 0 for deterministic structured outputs
    } = options;

    const body = {
      model,
      messages: [
        {
          role: 'user',
          content: prompt
        }
      ],
      stream: false,
      format: schema,  // Ollama's format parameter for constrained generation
      options: { temperature }
    };

    // Add system message if provided
    if (system) {
      body.messages.unshift({
        role: 'system',
        content: system
      });
    }

    try {
      const response = await fetch(`${this.baseUrl}/api/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(this.timeout)
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(`Ollama API error: ${response.status} - ${errorText}`);
      }

      const data = await response.json();
      const content = data.message?.content || data.response;
      
      // Ollama returns JSON string with format parameter
      return JSON.parse(content);
    } catch (error) {
      console.error('Structured generation failed:', error);
      throw error;
    }
  }
}

