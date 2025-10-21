console.log("metldr content script loaded")

document.addEventListener('mouseup', () => {
  const selectedText = window.getSelection().toString().trim();
  if (selectedText.length > 0) {
    try {
      if (!chrome.runtime?.id) {
        console.log('extension context lost, skipping message');
        return;
      }
      
      console.log("selected text:", selectedText)
      chrome.runtime.sendMessage({ type: 'SELECTION_TEXT', text: selectedText })
    }
    catch (e) {
      if (e.message.includes('Extension context invalidated')) {
        console.log('extension was reloaded, please refresh this page');
      } else {
        console.error(e);
      }
    }
  }
})
