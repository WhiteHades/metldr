# Features

metldr is being developed into a free browser assistant for complete tasks, reusable workflows and interactive answers. The lists below describe the intended finished app. Some parts already exist, while others still need implementation and verification. They are not a list of features available in the Chrome Web Store today.

## Available now

Read webpages and PDFs, summarize Gmail, draft replies, look up words and search page content. Use Chrome AI or an Ollama service. Ollama can offer local or cloud models. Open Help on the welcome page for setup.

The browser agent can read pages, navigate, click controls and fill fields. Approve browser changes, stop tasks and return to saved chats after a restart. An action with an uncertain result is not repeated automatically. Codex, OpenCode and Pi use their existing accounts and permissions. The current browser agent build needs the companion on Linux.

Connect a provider through its API key to run browser tasks or ask about a saved page. The selected provider processes the content needed for the task. Keys stay with the companion, outside page scripts. Supported connections include Messages, Responses, Google Interactions and Chat Completions.

## Planned browser and conversation features

* Work in your allowed signed in tabs.
* Read pages and inspect their controls.
* Capture screenshots and inspect page layouts.
* Navigate pages, history and multiple tabs.
* Group tabs by task or conversation.
* Track new tabs and browser popups.
* Click, hover and open context menus.
* Use single and double clicks.
* Drag, drop, scroll and select.
* Use keyboard shortcuts and the clipboard.
* Fill forms across several pages.
* Upload files and edit rich text.
* Run approved page scripts for complex tasks.
* Use page tools through WebMCP.
* Review plans and scoped approval rules.
* Set scoped automatic approvals for each session.
* Stop, pause and resume tasks.
* Verify results, redirects and dead links.
* See when browser control is active.
* Keep chats, attachments and live activity.
* Get contextual suggestions while browsing.
* Restore conversations and interrupted results.
* Send selected content to the sidebar.
* Open website prompts in the sidebar.
* Organize projects, goals and working directories.
* Continue work across multiple conversations.

## Planned everyday workflows

* Summarize pages, emails, PDFs and video transcripts.
* Triage inboxes and flag urgent messages.
* Draft follow up messages for unanswered threads.
* Research companies, suppliers and products.
* Compare prices, quotes and sources with citations.
* Find, qualify and track leads.
* Draft outreach and connection requests.
* Complete applications and surveys from documents.
* Extract invoices and reconcile payments.
* Screen applicants and update recruitment systems.
* Plan agendas, appointments and invitations.
* Build recurring reports across dashboards.
* Query BigQuery and connected data warehouses.
* Explain code, contracts and research.
* Create study notes and flashcards.
* Track orders, bookings and price changes.

## Planned reusable work and automation

* Teach workflows through instructions or demonstrations.
* Save versioned workflows and reusable templates.
* Set workflow inputs, outputs and approval rules.
* Run workflows manually or on schedules.
* Run workflows and Skills through slash commands.
* Set timezones, exceptions and overlap rules.
* Trigger workflows from page title changes.
* Pause, resume or remove scheduled work.
* Recover missed and interrupted scheduled runs.
* Keep tasks running with the panel closed.
* Track tasks, exceptions and run histories.
* Start and manage tasks from a dashboard.
* Create, import and share Skills.
* Import Skills from URLs or ZIP files.
* Browse built in, site and plugin Skills.
* Manage context files and editable preference memory.
* Inspect, edit, disable or delete stored memory.
* Scope corrections to projects or workflows.
* Optionally sync memory, projects, workflows and preferences.
* Manage team workspaces and member access.
* Set shared and individual usage budgets.
* Preview, edit and undo site scripts.
* List and remove saved site scripts.
* Run scripts automatically on matching pages.
* Reuse learned site tools and cached prompts.
* Add custom tools and dashboards to pages.
* Apply reading preferences and page shortcuts.
* Receive completion alerts and approval requests.

## Planned files and interactive answers

* Pin local files and folders to chats.
* Browse and manage approved working directories.
* Read and write files within approved locations.
* Edit Excel, CSV and browser spreadsheets.
* Read and write Google Sheets cell blocks.
* Format and edit browser documents.
* Preserve formulas and unrelated document content.
* Search chats, pages, files and documents.
* Read and edit Word documents.
* Use PDF text, OCR and dictionary lookup.
* Preview PDFs, images and generated files.
* Zoom and navigate PDF documents.
* Attach files, clipboard images and selected text.
* Copy images between allowed websites.
* Read large pages and files in chunks.
* Keep screenshot history during long tasks.
* Download folders as ZIP files.
* Download all task outputs together.
* Get tables, charts, diagrams, calculators and maps.
* Use interactive explanations and local controls.
* Stream answers and save reusable artifacts.
* Export results with screenshots.
* Record approved task videos for replay.
* Print pages and answers to PDF.

## Planned email, messaging and remote control

* Draft, reply, forward and send Gmail.
* Verify sent mail and approval outcomes.
* Read, search and manage WhatsApp chats.
* Send approved messages, files and reactions.
* Manage group members, roles and invite links.
* Control read status and conversation history.
* Dictate prompts and edit voice transcripts.
* Review or cancel voice recordings.
* Receive tasks through Telegram or WhatsApp.
* Send remote voice notes, photos and files.
* Receive results, screenshots and status updates.
* Approve remote actions and cancel work.
* Pair, switch and revoke devices or profiles.
* Let authorized MCP clients control tasks.

## Planned model choice, access and reliability

* Choose popular APIs and custom endpoints.
* Use local AI and browser models.
* Connect approved local and network model servers.
* Use API keys or supported account sign ins.
* Connect servers without keys where supported.
* Set model timeouts and connection settings.
* Switch models with visible capability limits.
* Run Codex, OpenCode, Pi and Claude Code.
* Keep agent accounts, tools, permissions and sessions.
* Connect remote MCP servers and tools.
* Select browser profiles through a CLI.
* Inspect token usage and estimated costs.
* Manage context with /compact and /usage.
* Recover connections without silent provider switching.
* Choose separate speech and task providers.
* Use multilingual and accessible interface controls.
* Set themes, text size and sound preferences.
* Keep site, tab and file permissions explicit.
* Isolate generated answers from browser authority.
* Export, delete and recover saved data.
* Preserve data through upgrades and interrupted work.
* Review uncertain writes before any repeat attempt.
* Keep local inference local unless you approve.
* Show model data destinations and access limits.
* Bound storage, memory and background work.
* Target Linux, Windows and macOS companions.
* Target Chrome, Edge, Brave and Chromebooks.
* Use every feature without an app subscription.

## Permissions and compatibility

Work must stay within the sites, tabs or tab groups, files, devices and profiles you allow. A saved Skill, correction or site tool does not grant extra access. Local model selection must not silently send work to a hosted service.

The intended scope excludes collecting passwords, payment card or bank credentials, solving or bypassing CAPTCHAs, and creating accounts. Chrome internal pages and other extensions' pages are unsupported. Chrome comes first. Edge, Brave and Chromebook compatibility are planned targets that need separate verification. Optional sync, team workspaces and remote connections need explicit access and data controls.

[Install and build](development.md) | [Privacy](../PRIVACY.md) | [Companion setup](../crates/metldr-companion/README.md)
