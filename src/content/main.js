console.log("metldr content script loaded")

document.addEventListener('mouseup', () => {
  const selectedText = window.getSelection().toString().trim();
  if (selectedText.length > 0) {
    console.log("selected text:", selectedText)
    chrome.runtime.sendMessage({ type: 'SELECTION_TEXT', text: selectedText })
  }
})
