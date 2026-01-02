import '@inboxsdk/core/background';
import { BackgroundBootstrap } from '../services/BackgroundBootstrap';

chrome.runtime.onInstalled.addListener((details) => {
  if (details.reason === 'install') {
    chrome.tabs.create({ url: chrome.runtime.getURL('welcome.html') });
  }
});

BackgroundBootstrap.init().catch(err => {
  console.error('metldr: failed to initialise background service:', err);
});
