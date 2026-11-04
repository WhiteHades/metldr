<p align="center">
  <img src="public/assets/logo/metldr-mark.svg" width="96" height="96" alt="metldr">
</p>
<h1 align="center">metldr</h1>
<p align="center">
  <a href="https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh"><img src="https://img.shields.io/badge/Chrome-extension-ff8338?style=flat-square" alt="Chrome extension"></a>
  <a href="docs/features.md"><img src="https://img.shields.io/badge/AI-your%20key%20or%20local-b4befe?style=flat-square" alt="Your API key or local AI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/App-free-75659a?style=flat-square" alt="Free app"></a>
</p>

metldr helps you read pages and get work done in Chrome with the AI you choose.

* Summarize webpages, PDFs and Gmail, ask questions and draft replies.
* Run browser tasks with approvals, Stop and saved chats.
* Use your provider, Codex, OpenCode, Pi, Chrome AI or Ollama.
* Look up words and search page content.

## Planned features

The finished app is intended to support the features below. Some parts already exist; others are still in development. This is the planned scope, not the current Chrome Web Store feature list.

### Browser work and conversations

- Work in your allowed signed-in tabs through natural-language instructions.
- Read pages, inspect controls and screenshots, navigate, click, type, scroll, select and drag across tabs.
- Fill forms and edit text fields, textareas and supported rich editors.
- Review plans, approve changes, set scoped session auto-approval rules and stop work.
- Verify actions, check link destinations, detect redirects and inspect uncertain outcomes.
- Keep saved chats, attachments, page suggestions, live activity and keyboard controls, and reopen saved results after a restart.
- Organize projects with goals, files, multiple conversations and saved progress.

### Tasks you can complete

- Research and compare products, prices and sources with citations.
- Find and qualify leads, gather evidence and prepare approved outreach.
- Fill applications and surveys from your documents.
- Extract products and invoices into spreadsheets, reconcile payouts and flag discrepancies.
- Screen applicants against saved criteria and update applicant tracking systems.
- Prepare calendar agendas, find available times and draft invitations.
- Build recurring reports from several dashboards with source links and visible exceptions.

### Workflows, Skills and memory

- Teach workflows through instructions or demonstrations, then save and revise their inputs, rules and output destinations.
- Run workflows manually, through slash commands, on schedules or from authorized remote clients.
- Reuse Skills, import them from validated URLs or ZIP files, and attach context files.
- Inspect, edit, disable and delete correction memory for a workflow or project.
- Preview, approve, edit and remove learned site tools, with URL matching and undo for supported changes.
- Schedule recurring work with timezones, exceptions, missed-run handling and overlap rules.
- Trigger approved workflows from scoped page-title changes.
- Track tasks, results, run histories and notifications in one dashboard.
- Cache stable prompts and reuse learned site actions for repeat work.

### Files and interactive answers

- Read and write approved local files and folders, spreadsheets, documents, PDFs and browser documents such as Sheets and Docs.
- Preserve page and email summaries, contextual questions, citations, PDF text, OCR and dictionary lookup.
- Download, edit and reuse generated files.
- Get answers as tables, charts, diagrams, calculators, maps and interactive explanations.
- Use streaming answers and local controls, send deliberate follow-ups, save artifacts and export results.
- Discover and use tools offered by websites through WebMCP.

### Email, voice and remote control

- Draft and send approved Gmail messages, then verify the result in Sent.
- Read and search WhatsApp Web chats, send approved text or files, add reactions and manage read status.
- Support approved WhatsApp group member and administrator actions.
- Dictate instructions, edit transcripts and use browser shortcuts.
- Pair messaging or Model Context Protocol clients to start tasks, retrieve results and answer approvals remotely.
- Select authorized devices and browser profiles, or control work through a CLI.

### Model choice and reliability

- Choose popular provider APIs, custom endpoints, browser models and useful local inference.
- Run Codex, OpenCode and Pi with their existing accounts, tools, permissions and continuing sessions.
- See model capabilities, data destinations, token usage and provider costs, with no silent local-to-cloud fallback.
- Keep site, tab, file and remote-access permissions explicit, and isolate generated answers from browser authority.
- Preserve saved data through upgrades and recovery, and avoid repeating uncertain external writes.
- Use accessible keyboard and screen-reader controls, themes, text sizes and sound preferences.
- Use one Chrome extension with companion support on Linux, Windows and macOS.
- Keep background work, storage and memory bounded, with measured performance improvements.
- Use the full app without an application subscription or feature paywall. Model providers and optional services may charge.

See the [feature guide](docs/features.md) for current availability, planned details and compatibility limits.

[Install from the Chrome Web Store](https://chromewebstore.google.com/detail/kbfdmfgmmibkdnbfdaeganjckfgbfjlh).
The app is free. Your model provider may charge.

The browser agent is in development. Its current build needs Linux and [companion setup](crates/metldr-companion/README.md).

[Build and setup](docs/development.md) | [Privacy](PRIVACY.md)
