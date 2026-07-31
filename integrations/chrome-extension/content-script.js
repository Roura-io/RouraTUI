const CURSOR_ID = "__rouratui_cursor";
const HIGHLIGHT_ID = "__rouratui_highlight";
let cursorPosition = { x: window.innerWidth / 2, y: window.innerHeight / 2 };

function ensureCursor() {
  let cursor = document.getElementById(CURSOR_ID);
  if (cursor) return cursor;

  cursor = document.createElement("div");
  cursor.id = CURSOR_ID;
  Object.assign(cursor.style, {
    position: "fixed",
    left: "0",
    top: "0",
    width: "22px",
    height: "28px",
    zIndex: "2147483647",
    pointerEvents: "none",
    transform: `translate(${cursorPosition.x}px, ${cursorPosition.y}px)`,
    filter: "drop-shadow(0 2px 4px rgba(0,0,0,.45))",
    transition: "filter 120ms ease",
  });
  cursor.innerHTML = `
    <svg viewBox="0 0 22 28" width="22" height="28" aria-hidden="true">
      <path d="M2 1.5v20.2l5.3-5.1 3.7 8.7 3.7-1.6-3.6-8.5h7.5L2 1.5Z"
        fill="#D97757" stroke="white" stroke-width="1.5" stroke-linejoin="round"/>
    </svg>`;
  document.documentElement.appendChild(cursor);
  return cursor;
}

function ensureHighlight() {
  let highlight = document.getElementById(HIGHLIGHT_ID);
  if (highlight) return highlight;
  highlight = document.createElement("div");
  highlight.id = HIGHLIGHT_ID;
  Object.assign(highlight.style, {
    position: "fixed",
    zIndex: "2147483646",
    pointerEvents: "none",
    border: "2px solid #D97757",
    borderRadius: "7px",
    boxShadow: "0 0 0 3px rgba(217,119,87,.22), 0 8px 24px rgba(0,0,0,.18)",
    opacity: "0",
    transition: "all 150ms ease, opacity 120ms ease",
  });
  document.documentElement.appendChild(highlight);
  return highlight;
}

function interactiveElements() {
  const selector = [
    "a[href]", "button", "input", "textarea", "select", "summary",
    "[role='button']", "[role='link']", "[role='textbox']", "[tabindex]"
  ].join(",");
  return [...document.querySelectorAll(selector)]
    .filter((element) => {
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
    })
    .slice(0, 500)
    .map((element, index) => {
      const id = element.dataset.rouratuiId || `rt-${Date.now()}-${index}`;
      element.dataset.rouratuiId = id;
      const rect = element.getBoundingClientRect();
      return {
        id,
        tag: element.tagName.toLowerCase(),
        role: element.getAttribute("role") || "",
        name: element.getAttribute("aria-label") || element.innerText?.trim().slice(0, 180) || element.getAttribute("placeholder") || element.getAttribute("name") || "",
        type: element.getAttribute("type") || "",
        href: element.href || "",
        bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      };
    });
}

function findTarget(id) {
  return document.querySelector(`[data-rouratui-id="${CSS.escape(id)}"]`);
}

async function moveCursor(x, y, duration = 550) {
  const cursor = ensureCursor();
  const start = { ...cursorPosition };
  const startedAt = performance.now();
  await new Promise((resolve) => {
    function frame(now) {
      const progress = Math.min(1, (now - startedAt) / Math.max(1, duration));
      const eased = 1 - Math.pow(1 - progress, 3);
      cursorPosition = {
        x: start.x + (x - start.x) * eased,
        y: start.y + (y - start.y) * eased,
      };
      cursor.style.transform = `translate(${cursorPosition.x}px, ${cursorPosition.y}px)`;
      if (progress < 1) requestAnimationFrame(frame); else resolve();
    }
    requestAnimationFrame(frame);
  });
}

async function pointAt(element) {
  element.scrollIntoView({ block: "center", inline: "center", behavior: "smooth" });
  await new Promise((resolve) => setTimeout(resolve, 300));
  const rect = element.getBoundingClientRect();
  const highlight = ensureHighlight();
  Object.assign(highlight.style, {
    left: `${rect.left - 4}px`, top: `${rect.top - 4}px`,
    width: `${rect.width + 8}px`, height: `${rect.height + 8}px`, opacity: "1",
  });
  await moveCursor(rect.left + rect.width / 2, rect.top + rect.height / 2);
  return rect;
}

async function visibleClick(element) {
  await pointAt(element);
  const cursor = ensureCursor();
  cursor.style.filter = "drop-shadow(0 0 8px #D97757) scale(.86)";
  await new Promise((resolve) => setTimeout(resolve, 130));
  element.click();
  cursor.style.filter = "drop-shadow(0 2px 4px rgba(0,0,0,.45))";
  await new Promise((resolve) => setTimeout(resolve, 180));
  ensureHighlight().style.opacity = "0";
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  (async () => {
    if (message.type === "snapshot") return { url: location.href, title: document.title, elements: interactiveElements() };
    if (message.type === "point") {
      const element = findTarget(message.id);
      if (!element) throw new Error(`Element ${message.id} is no longer available`);
      await pointAt(element);
      return { ok: true };
    }
    if (message.type === "click") {
      const element = findTarget(message.id);
      if (!element) throw new Error(`Element ${message.id} is no longer available`);
      await visibleClick(element);
      return { ok: true, url: location.href };
    }
    if (message.type === "type") {
      const element = findTarget(message.id);
      if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element.isContentEditable)) {
        throw new Error(`Element ${message.id} is not editable`);
      }
      await pointAt(element);
      element.focus();
      if ("value" in element) element.value = message.text; else element.textContent = message.text;
      element.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: message.text }));
      ensureHighlight().style.opacity = "0";
      return { ok: true };
    }
    throw new Error(`Unsupported content command: ${message.type}`);
  })().then(sendResponse).catch((error) => sendResponse({ ok: false, error: error.message }));
  return true;
});

ensureCursor();
