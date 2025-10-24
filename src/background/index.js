// background service worker handling ollama communication and message routing

chrome.sidePanel
  .setPanelBehavior({ openPanelOnActionClick: true })
  .catch((error) => console.error('Side panel error:', error));

// TODO: milestone 2, message handler, expanded for email processing
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === 'SUMMARIZE_EMAIL') {
    sendResponse({ error: 'Not implemented yet' });
  }
  
  return true;
});