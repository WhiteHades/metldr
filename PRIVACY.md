# Privacy

Updated 22 October 2026.

## Where content goes

Local models process content on your computer. Ollama can also offer cloud models, so check the model you select.

Provider connections send task instructions, page content and relevant prior answers to your configured endpoint. Installed agents receive task instructions and supplied page context, using their configured accounts and models. Their tools and metldr's browser tools can provide additional access, subject to the agent's permissions and metldr's browser approvals. Your provider's and agent's policies apply to that processing.

Dictionary lookups can send selected words to dictionaryapi.dev. Downloads contact the services that supply the requested files.

## What stays saved

The extension saves settings, cached summaries and page context in your browser. The companion saves conversations, results and recovery records on your computer. Installed agents can keep their own sessions and files.

Provider keys are managed by the companion's credential store, outside page scripts. Installed agents keep their own authentication.

Clearing the extension's cache or browser data does not remove companion records or agent files. Remove those separately using their storage locations and tools. See [companion setup](crates/metldr-companion/README.md) for its data location.

## Browser access

Page permissions let metldr add its controls and read content for its features. The browser agent asks for approval before supported page changes. Installed agents also have their own permission rules.

Review your selected connection and permissions before sharing private content. For questions, use the support contact on the [Chrome Web Store listing](https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh).
