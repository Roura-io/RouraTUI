const NATIVE_HOST = "com.roura_io.rouratui";
let nativePort;

function activeTab() {
  return chrome.tabs.query({ active: true, lastFocusedWindow: true }).then(([tab]) => {
    if (!tab?.id) throw new Error("No active Chrome tab");
    return tab;
  });
}

async function targetTab(command) {
  const tab = Number.isInteger(command.tabId)
    ? await chrome.tabs.get(command.tabId)
    : await activeTab();
  if (!tab?.id) throw new Error(`Chrome tab ${command.tabId ?? "active"} is unavailable`);
  if (command.focus === true) {
    if (tab.windowId != null) await chrome.windows.update(tab.windowId, { focused: true });
    await chrome.tabs.update(tab.id, { active: true });
  }
  return tab;
}

async function contentCommand(command) {
  const tab = await targetTab(command);
  const response = await chrome.tabs.sendMessage(tab.id, command);
  return { tabId: tab.id, ...response };
}

async function handleCommand(command) {
  switch (command.type) {
    case "status": {
      const tab = await activeTab();
      return { ok: true, tab: { id: tab.id, title: tab.title, url: tab.url } };
    }
    case "tabs": {
      const tabs = await chrome.tabs.query({ currentWindow: true });
      return { ok: true, tabs: tabs.map(({ id, active, title, url }) => ({ id, active, title, url })) };
    }
    case "navigate": {
      const tab = await targetTab(command);
      await chrome.tabs.update(tab.id, { url: command.url });
      return { ok: true, tabId: tab.id, url: command.url };
    }
    case "snapshot":
    case "point":
    case "click":
    case "type":
      return contentCommand(command);
    default:
      throw new Error(`Unsupported browser command: ${command.type}`);
  }
}

// broadcastHide — rouratui isn't driving anything right now; clear any cursor
// still fading out from a moment ago so the page never looks "controlled"
// while nothing is connected. Tabs without the content script (chrome://,
// the Web Store, etc.) reject the message — that's expected, ignore it.
async function broadcastHide() {
  const tabs = await chrome.tabs.query({});
  for (const tab of tabs) {
    if (tab.id != null) chrome.tabs.sendMessage(tab.id, { type: "hide" }).catch(() => undefined);
  }
}

function connectNativeHost() {
  if (nativePort) return;
  try {
    nativePort = chrome.runtime.connectNative(NATIVE_HOST);
    nativePort.onMessage.addListener((command) => {
      handleCommand(command)
        .then((result) => nativePort.postMessage({ requestId: command.requestId, ...result }))
        .catch((error) => nativePort.postMessage({ requestId: command.requestId, ok: false, error: error.message }));
    });
    nativePort.onDisconnect.addListener(() => {
      nativePort = undefined;
      broadcastHide(); // rouratui just disconnected — don't leave a stray cursor on screen
      setTimeout(connectNativeHost, 1000);
    });
  } catch (error) {
    console.debug("RouraTUI native host is not installed yet", error);
    nativePort = undefined;
    setTimeout(connectNativeHost, 2000);
  }
}

chrome.runtime.onInstalled.addListener(connectNativeHost);
chrome.runtime.onStartup.addListener(connectNativeHost);
chrome.action.onClicked.addListener(async () => {
  if (!nativePort) connectNativeHost();
  const tab = await activeTab();
  chrome.tabs.sendMessage(tab.id, { type: "snapshot" }).catch(() => undefined);
});

connectNativeHost();
