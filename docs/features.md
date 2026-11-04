# Features

metldr is being developed into a free browser assistant for complete tasks, reusable workflows and interactive answers. The lists below describe the intended finished app. Some parts already exist, while others still need implementation and verification. They are not a list of features available in the Chrome Web Store today.

## Available now

Read webpages and PDFs, summarize Gmail, draft replies, look up words and search page content. Use Chrome AI or an Ollama service. Ollama can offer local or cloud models. Open Help on the welcome page for setup.

The browser agent can read pages, navigate, click controls and fill fields. Approve browser changes, stop tasks and return to saved chats after a restart. An action with an uncertain result is not repeated automatically. Codex, OpenCode and Pi use their existing accounts and permissions. The current browser agent build needs the companion on Linux.

Connect a provider through its API key to run browser tasks or ask about a saved page. The selected provider processes the content needed for the task. Keys stay with the companion, outside page scripts. Supported connections include Messages, Responses, Google Interactions and Chat Completions.

## Planned browser and conversation features

- Give instructions in ordinary language and work in the signed-in tabs you allow.
- Read page structure, inspect controls and screenshots, navigate, click, type, scroll, select and drag across allowed tabs.
- Fill forms and edit text fields, textareas and supported rich editors with a preview of consequential changes.
- Review a plan and approve actions individually or through scoped session auto-approval rules.
- Verify what happened after an action, check link destinations and liveness, detect changed pages and redirects, and show uncertain results before another attempt.
- Stop active work, retain completed results and recover interrupted tasks without repeating uncertain writes.
- Keep saved conversations, attachments, page suggestions and a live activity stream, with keyboard controls for opening and stopping work. Reopen conversations and recover saved results after a restart.
- Organize projects with goals, files, several conversations and saved progress.
- Discover website-provided WebMCP tools and route their results through the same task controls.

## Planned everyday workflows

- Summarize pages, emails and PDFs, then ask follow-up questions with citations to the saved sources.
- Compare products, prices and research sources while preserving currencies, billing periods and missing information.
- Find and qualify leads, gather supporting evidence and prepare approved outreach or connection requests.
- Fill applications and surveys from your resume, company information or attached documents.
- Extract products, prices and invoice details into spreadsheets.
- Reconcile payouts against orders and flag missing payments, currency differences and other discrepancies.
- Screen applicants against saved criteria and update applicant tracking systems, with approval for consequential changes.
- Prepare calendar agendas, find available times and draft invitations with the correct timezone and participants.
- Build recurring reports from several dashboards, with saved templates, source links and visible gaps when a source is unavailable.

## Planned reusable work and automation

- Teach a workflow through instructions or a deliberate demonstration in selected tabs, then review it before saving.
- Save versioned workflows with variable inputs, allowed sites and files, output destinations, model choices and approval rules.
- Run a saved workflow manually, through a slash command, on a schedule or from an authorized remote client.
- Save reusable Skills, import them from validated URLs or ZIP files, and attach context files.
- Inspect, edit, disable and delete correction memory scoped to a workflow or project.
- Generate, preview, approve, edit and remove site tools. Run them on matching URLs and undo supported changes.
- Schedule recurring work with timezones, exceptions, missed-run handling and rules for overlapping runs.
- Start approved workflows from scoped page-title triggers, with visible pause and removal controls.
- Follow tasks through a dashboard, inspect run histories and results, and receive notifications for failures or decisions that need attention.
- Cache stable prompts and reuse learned site actions for repeat work, with usage and cost information where the provider supplies it.

## Planned files and interactive answers

- Read and write selected local files and folders, with access limited to the destinations you approve.
- Work with spreadsheets, documents and PDFs, including Excel and CSV files and browser documents such as Google Sheets and Docs. Each format needs its own supported operations and preservation checks.
- Extract PDF text, recognize text in scanned documents and preserve dictionary lookup and language support.
- Download, edit and reuse files produced by a task.
- Present answers as text, tables, charts, diagrams, calculators, maps and interactive explanations.
- Stream answers as they are produced, use local controls and deliberately turn an answer into a follow-up request.
- Save answer artifacts, export useful results and reopen them after a restart.
- Keep generated answer code isolated from credentials, extension APIs and browser automation permissions.

## Planned email, messaging and remote control

- Draft and send approved Gmail messages with recipients, cc, bcc, subject and plain-text or HTML bodies, then verify the result in Sent.
- List, search and read WhatsApp Web chats, send approved text or files, add reactions and manage read status.
- Support approved WhatsApp group member and administrator actions.
- Dictate instructions in the sidebar, edit the transcript and use a browser shortcut. Speech service options may need their own provider key.
- Pair authorized messaging or Model Context Protocol clients to start tasks, retrieve results and answer approvals remotely.
- Select the intended device and browser profile for remote work, and revoke access when it is no longer needed.

## Planned model choice, access and reliability

- Choose popular provider APIs, compatible custom endpoints, browser models or local inference, with visible capability differences.
- Run installed Codex, OpenCode and Pi normally, preserving their accounts, tools, settings, permissions and continuing sessions.
- Use a practical local model path with clear model-download and hardware requirements, without silent fallback to a remote provider.
- Control tasks through a CLI and select among explicitly authorized browser profiles.
- See token usage and provider costs, with estimates distinguished from reported charges.
- Use familiar themes, text sizes and sound preferences, with keyboard, focus, screen-reader and input-method support.
- Preserve saved data through upgrades and recovery, with export, deletion and clear compatibility limits.
- Use the Chrome extension and companion on Linux, Windows and macOS. Native Windows and macOS support still needs verification.
- Keep the app responsive through efficient Rust execution, bounded background work, storage and memory use. Performance gains need measurements before they can be claimed.
- Use the full app without an application subscription or feature paywall. External providers and optional services may charge.

## Permissions and compatibility

Work must stay within the sites, tabs or tab groups, files, devices and profiles you allow. A saved Skill, correction or site tool does not grant extra access. Local model selection must not silently send work to a hosted service.

The intended scope excludes collecting passwords, payment-card or bank credentials, solving or bypassing CAPTCHAs, and creating accounts. Chrome internal pages and other extensions' pages are unsupported. Chrome comes first; other browser and Chromebook compatibility must be documented and verified separately.

[Install and build](development.md) | [Privacy](../PRIVACY.md) | [Companion setup](../crates/metldr-companion/README.md)
