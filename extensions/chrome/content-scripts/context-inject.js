// Sage Context Inject — floating button to prepend Sage memories into AI chat inputs
// Runs on chat pages only (claude.ai, chatgpt.com, gemini.google.com)

(function () {
  "use strict";

  // ── Page detection ────────────────────────────────────────────────────────

  function detectChatPage() {
    const { hostname, pathname } = location;

    if (hostname === "claude.ai") {
      return (pathname === "/" || pathname.startsWith("/chat")) &&
        !pathname.startsWith("/settings");
    }

    if (hostname === "chatgpt.com" || hostname === "chat.openai.com") {
      return (pathname === "/" || pathname.startsWith("/c/")) &&
        !pathname.startsWith("/g/") &&
        !pathname.startsWith("/auth");
    }

    if (hostname === "gemini.google.com") {
      return pathname.startsWith("/app") &&
        !pathname.startsWith("/app/settings");
    }

    return false;
  }

  // ── Input adapters ────────────────────────────────────────────────────────

  function getInputAdapter() {
    const { hostname } = location;

    if (hostname === "claude.ai") {
      return {
        find: () => document.querySelector('div.ProseMirror[contenteditable="true"]'),
        inject: (el, text) => {
          el.innerText = text + "\n\n" + el.innerText;
          el.dispatchEvent(new InputEvent("input", { bubbles: true }));
        },
      };
    }

    if (hostname === "chatgpt.com" || hostname === "chat.openai.com") {
      return {
        find: () =>
          document.querySelector("#prompt-textarea") ||
          document.querySelector("textarea"),
        inject: (el, text) => {
          el.focus();
          const inserted = document.execCommand
            ? document.execCommand("insertText", false, text + "\n\n")
            : false;
          if (!inserted) {
            el.innerText = text + "\n\n" + (el.innerText || el.value || "");
            el.dispatchEvent(new InputEvent("input", { bubbles: true }));
          }
        },
      };
    }

    if (hostname === "gemini.google.com") {
      return {
        find: () =>
          document.querySelector('div.ql-editor[contenteditable="true"]') ||
          document.querySelector('rich-textarea div[contenteditable]'),
        inject: (el, text) => {
          el.innerText = text + "\n\n" + el.innerText;
          el.dispatchEvent(new InputEvent("input", { bubbles: true }));
        },
      };
    }

    // Fallback
    return {
      find: () => document.querySelector('[contenteditable="true"]'),
      inject: (el, text) => {
        el.innerText = text + "\n\n" + el.innerText;
        el.dispatchEvent(new InputEvent("input", { bubbles: true }));
      },
    };
  }

  // ── Modal ─────────────────────────────────────────────────────────────────

  function showPreviewModal(contextText, onConfirm) {
    const overlay = document.createElement("div");
    overlay.className = "sage-modal-overlay";

    overlay.innerHTML = `
      <div class="sage-modal">
        <div class="sage-modal-header">
          <span class="sage-modal-title">Sage Context Preview</span>
        </div>
        <pre class="sage-modal-body">${escapeHtml(contextText)}</pre>
        <div class="sage-modal-footer">
          <button class="sage-btn sage-btn-cancel">Cancel</button>
          <button class="sage-btn sage-btn-inject">Inject</button>
        </div>
      </div>
    `;

    injectStyles();
    document.body.appendChild(overlay);

    overlay.querySelector(".sage-btn-cancel").addEventListener("click", () => {
      overlay.remove();
    });

    overlay.querySelector(".sage-btn-inject").addEventListener("click", () => {
      overlay.remove();
      onConfirm();
    });

    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) overlay.remove();
    });
  }

  function escapeHtml(str) {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  // ── Toast ─────────────────────────────────────────────────────────────────

  function showToast(message) {
    injectStyles();
    const toast = document.createElement("div");
    toast.className = "sage-toast";
    toast.textContent = message;
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 3000);
  }

  // ── Floating button ───────────────────────────────────────────────────────

  function createFloatingButton() {
    if (document.getElementById("sage-inject-btn")) return;

    injectStyles();

    const btn = document.createElement("button");
    btn.id = "sage-inject-btn";
    btn.title = "Inject Sage context";
    btn.innerHTML = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="12" cy="12" r="3"/><path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"/>
    </svg>`;
    document.body.appendChild(btn);

    btn.addEventListener("click", () => {
      btn.disabled = true;
      chrome.runtime.sendMessage(
        { type: "FETCH_CONTEXT", payload: { limit: 10 } },
        (response) => {
          btn.disabled = false;
          if (!response || !response.ok) {
            showToast("Sage offline — context unavailable");
            return;
          }
          const contextText = formatContext(response.data);
          const adapter = getInputAdapter();
          showPreviewModal(contextText, () => {
            const el = adapter.find();
            if (el) {
              adapter.inject(el, contextText);
            } else {
              showToast("Could not find input field");
            }
          });
        }
      );
    });
  }

  function formatContext(data) {
    if (typeof data === "string") return data;
    if (data?.context) return data.context;
    if (Array.isArray(data)) {
      return data.map((m, i) => `${i + 1}. ${m.content || m}`).join("\n");
    }
    return JSON.stringify(data, null, 2);
  }

  // ── Styles (injected once) ────────────────────────────────────────────────

  let _stylesInjected = false;

  function injectStyles() {
    if (_stylesInjected) return;
    _stylesInjected = true;

    const style = document.createElement("style");
    style.textContent = `
      #sage-inject-btn {
        position: fixed; bottom: 24px; right: 24px; z-index: 99999;
        width: 40px; height: 40px; border-radius: 50%; border: none;
        background: #22c55e; color: #fff; cursor: pointer;
        display: flex; align-items: center; justify-content: center;
        box-shadow: 0 2px 8px rgba(0,0,0,0.25);
        transition: background 0.15s, transform 0.1s;
      }
      #sage-inject-btn:hover { background: #16a34a; transform: scale(1.08); }
      #sage-inject-btn:disabled { opacity: 0.6; cursor: wait; }

      .sage-modal-overlay {
        position: fixed; inset: 0; z-index: 100000;
        background: rgba(0,0,0,0.45); display: flex;
        align-items: center; justify-content: center;
      }
      .sage-modal {
        background: #1e1e2e; color: #cdd6f4; border-radius: 10px;
        width: min(560px, 90vw); max-height: 70vh;
        display: flex; flex-direction: column;
        box-shadow: 0 8px 32px rgba(0,0,0,0.5);
        font-family: system-ui, sans-serif; font-size: 14px;
      }
      .sage-modal-header {
        padding: 14px 18px; border-bottom: 1px solid #313244;
      }
      .sage-modal-title { font-weight: 600; font-size: 15px; }
      .sage-modal-body {
        flex: 1; overflow-y: auto; margin: 0;
        padding: 14px 18px; white-space: pre-wrap; word-break: break-word;
        font-size: 13px; color: #a6e3a1; font-family: monospace;
        background: #181825; border: none;
      }
      .sage-modal-footer {
        padding: 12px 18px; display: flex; justify-content: flex-end;
        gap: 10px; border-top: 1px solid #313244;
      }
      .sage-btn {
        padding: 7px 18px; border-radius: 6px; border: none;
        cursor: pointer; font-size: 13px; font-weight: 500;
      }
      .sage-btn-cancel { background: #313244; color: #cdd6f4; }
      .sage-btn-cancel:hover { background: #45475a; }
      .sage-btn-inject { background: #22c55e; color: #fff; }
      .sage-btn-inject:hover { background: #16a34a; }

      .sage-toast {
        position: fixed; bottom: 76px; right: 24px; z-index: 100001;
        background: #1e1e2e; color: #f38ba8; border: 1px solid #f38ba8;
        border-radius: 8px; padding: 9px 16px;
        font-family: system-ui, sans-serif; font-size: 13px;
        box-shadow: 0 2px 10px rgba(0,0,0,0.3);
        animation: sage-fadein 0.2s ease;
      }
      @keyframes sage-fadein { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; } }
    `;
    document.head.appendChild(style);
  }

  // ── Entry point ───────────────────────────────────────────────────────────

  if (detectChatPage()) {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", createFloatingButton);
    } else {
      createFloatingButton();
    }
  }
})();
