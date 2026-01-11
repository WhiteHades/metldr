# Privacy Policy for metldr

**Effective Date:** January 11, 2026

## 1. Introduction

**metldr** is a comprehensive, privacy-first AI productivity extension for Chrome. It enhances your browsing workflow by providing:

- **Email Intelligence:** Automatically summarises emails and suggests replies directly within Gmail.
- **Page Intelligence:** Generates concise bullet-point summaries of web articles and allows you to "Chat with Page" to ask questions about the content.
- **Dictionary:** Instant definitions for selected words, working offline.
- **Local RAG (Retrieval-Augmented Generation):** Indexes your viewed content locally to provide answers based on your content.

All these features are built on a "Local-First" architecture. We believe your data belongs to you, which is why metldr is designed to run its AI models entirely on your device (via Chrome Built-in AI or a local Ollama instance).

## 2. Data Collection and Usage

### 2.1. Local Processing (No Cloud Collection)

metldr processes data locally on your computer.

- **AI Processing:** All text analysis (summarisation, reply generation, chat) happens on your device using either Chrome's Gemini Nano or your local Ollama setup.
- **Vector Database:** Your "Chat with Page" context is stored in a local vector database (Voy) inside your browser.
- **No Analytics:** We do not track your usage behavior, clicks, or browsing history for analytics purposes.

**We do not operate any servers that receive, store, or analyse your content.**

### 2.2. Permissions Usage

To function, metldr requires specific permissions:

- **Read Content (`<all_urls>`):** This permission is used _only_ when you explicitly use the extension (e.g., open the specific email summary or sidebar). It allows the extension to read the text of the page to generate the summary. This text remains in your browser's memory and is not sent to us.
- **Storage:** Used to save your settings, cached summaries, and offline dictionary components.

### 2.3. Third-Party Services

metldr connects to the following third-party services only for specific resource retrieval:

- **GitHub (`media.githubusercontent.com`):**
  - **Purpose:** Used to download **offline dictionary packs**.
  - **Data:** No user data is sent. The extension only performs GET requests to fetch these static public files.

- **Dictionary API (`api.dictionaryapi.dev`):**
  - **Purpose:** Used for the word meaning lookup.
  - **Data:** Only the single word you are defining is sent. No context or user attributes are transmitted.

### 2.4. Ollama (Optional Local Connection)

If you choose to use Ollama, metldr connects to `http://localhost:11434`. This is a connection to **your own computer**, not the internet. Your data remains strictly local.

## 3. Data Retention

Since we do not collect your data, we do not retain it.

- **Browser Cache:** Summaries and chat history stored in `IndexedDB` on your device can be cleared via the extension's "Clear Cache" button in settings or by clearing browser data.
- **Vector Index:** The search index for "Chat with Page" persists locally but can be reset at any time.

## 4. Updates to this Policy

We may update this privacy policy to reflect changes in our practices. If we make significant changes, we will notify you through the extension's update notes.

## 5. Contact

If you have questions about this policy or the extension's practices, please contact the developer via the support link on the Chrome Web Store listing.
