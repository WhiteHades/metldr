let sidePanelPort = null;

// side panel behaviour
chrome.sidePanel
  .setPanelBehavior({ openPanelOnActionClick: true })
  .catch((error) => console.error(error));

// connections from the side panel
chrome.runtime.onConnect.addListener((port) => {
  if (port.name === "side_panel") {
    sidePanelPort = port;

    port.onMessage.addListener((msg) => {
      console.log("message from side panel:", msg);
    });

    port.onDisconnect.addListener(() => {
      sidePanelPort = null;
    });
  }
});

// messages from content scripts
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  console.log("received message from content script:", message);

  if (sidePanelPort) {
    sidePanelPort.postMessage(message);
  }
});
