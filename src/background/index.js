let sidePanelPort = null;
let lastSelectionMessage = null;

// side panel behaviour
chrome.sidePanel
  .setPanelBehavior({ openPanelOnActionClick: true })
  .catch((error) => console.error(error));

// connections from the side panel
chrome.runtime.onConnect.addListener((port) => {
  if (port.name === "side_panel") {
    sidePanelPort = port;
    console.log("side panel connected")

    if (lastSelectionMessage) {
      console.log("sending stored message to newly connected side panel:", lastSelectionMessage)
      sidePanelPort.postMessage(lastSelectionMessage);
    }

    port.onMessage.addListener((msg) => {
      console.log("message from side panel:", msg);
    });

    port.onDisconnect.addListener(() => {
      sidePanelPort = null;
      console.log("side panel disconnected");
    });
  }
});

// messages from content scripts
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  console.log("received message from content script:", message);

  if (message.type === 'SELECTION_TEXT') {
    lastSelectionMessage = message;
  }

  if (sidePanelPort) {
    sidePanelPort.postMessage(message);
  }
});
