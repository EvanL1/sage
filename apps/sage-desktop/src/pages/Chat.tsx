import { useState, useRef, useEffect, useCallback } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { formatDate } from "../utils/time";
import { useLang } from "../LangContext";

interface ChatMessage {
  id: number;
  role: "user" | "sage";
  content: string;
  session_id: string;
  created_at: string;
}

interface ChatSession {
  session_id: string;
  preview: string;
  message_count: number;
  created_at: string;
  updated_at: string;
}

function formatSessionDate(ts: string): string {
  return formatDate(ts, "short");
}

function Chat() {
  const { t } = useLang();
  const location = useLocation();
  const navigate = useNavigate();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [reflecting, setReflecting] = useState(false);
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [showSessions, setShowSessions] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const lastLocationKey = useRef("");
  const msgCountRef = useRef(0);
  const composingRef = useRef(false);
  const [quote, setQuote] = useState<string | null>(null);

  // Scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  // Focus input
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const loadSessions = useCallback(() => {
    invoke<ChatSession[]>("list_chat_sessions", { limit: 30 })
      .then(setSessions)
      .catch(console.error);
  }, []);

  // Load chat history
  useEffect(() => {
    invoke<ChatMessage[]>("get_chat_history", { limit: 50 })
      .then((history) => {
        if (history.length > 0) {
          setMessages(history.map((m) => ({
            ...m,
            role: m.role as "user" | "sage",
          })));
          setSessionId(history[history.length - 1].session_id);
          msgCountRef.current = history.length;
        }
      })
      .catch(console.error);
    loadSessions();
  }, [loadSessions]);

  const loadSession = async (sid: string) => {
    try {
      const history = await invoke<ChatMessage[]>("get_chat_history", { sessionId: sid });
      setMessages(history.map((m) => ({ ...m, role: m.role as "user" | "sage" })));
      setSessionId(sid);
      setShowSessions(false);
      inputRef.current?.focus();
    } catch (err) {
      console.error("Failed to load session:", err);
    }
  };

  const triggerMemoryExtraction = async (sid: string) => {
    setReflecting(true);
    try {
      await invoke("extract_memories", { sessionId: sid });
    } catch (err) {
      console.error("Memory extraction failed:", err);
    } finally {
      setTimeout(() => setReflecting(false), 2000);
    }
  };

  const startNewSession = () => {
    if (sessionId && messages.length >= 4) {
      triggerMemoryExtraction(sessionId);
    }
    setSessionId(null);
    setMessages([]);
    msgCountRef.current = 0;
    loadSessions();
  };

  const sendMessage = async (overrideText?: string, forceNewSession?: boolean) => {
    const rawText = (overrideText ?? input).trim();
    if (!rawText || loading) return;

    // Build quoted blockquote if needed
    const text = quote
      ? `> ${quote.replace(/\n/g, "\n> ")}\n\n${rawText}`
      : rawText;

    const sid = forceNewSession ? null : sessionId;

    const tempUserMsg: ChatMessage = {
      id: Date.now(),
      role: "user",
      content: text,
      session_id: sid || "",
      created_at: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, tempUserMsg]);
    setQuote(null);
    setInput("");
    setLoading(true);

    try {
      const result = await invoke<{ response: string; session_id: string; cancelled?: boolean; page_id?: number }>("chat", {
        message: text,
        sessionId: sid,
      });

      if (result.cancelled) return;

      if (!sessionId) {
        setSessionId(result.session_id);
      }

      const sageMsg: ChatMessage = {
        id: Date.now() + 1,
        role: "sage",
        content: result.response,
        session_id: result.session_id,
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, sageMsg]);

      // If backend generated a dynamic page, show a navigation card
      if (result.page_id) {
        const pageCard: ChatMessage = {
          id: Date.now() + 2,
          role: "sage",
          content: `📄 [${t("pages.created")} — ${t("pages.title")}](/pages/${result.page_id})`,
          session_id: result.session_id,
          created_at: new Date().toISOString(),
        };
        setMessages((prev) => [...prev, pageCard]);
        // Auto-navigate after a short delay so user sees the card first
        setTimeout(() => navigate(`/pages/${result.page_id}`), 1500);
      }

      // Trigger memory extraction every 4 messages (2 rounds)
      msgCountRef.current += 2;
      if (msgCountRef.current % 4 === 0) {
        triggerMemoryExtraction(result.session_id);
      }

      loadSessions();
    } catch (err) {
      const errStr = String(err);
      if (errStr.includes("cancelled")) return;
      // 只有明确"没有可用的 AI 服务"/"No AI provider"才显示配置提示，其他错误显示原文
      const isNoProvider = errStr.includes("没有可用的 AI") || errStr.includes("No AI provider");
      const errorContent = isNoProvider
        ? t("chat.providerError")
        : t("chat.errorPrefix") + errStr;
      const errorMsg: ChatMessage = {
        id: Date.now() + 1,
        role: "sage",
        content: errorContent,
        session_id: sessionId || "",
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, errorMsg]);
      console.error(err);
    } finally {
      setLoading(false);
      inputRef.current?.focus();
    }
  };

  // Handle navigation from Dashboard/History
  useEffect(() => {
    const state = location.state as { initialMessage?: string; prefill?: string; quote?: string } | null;
    if (!state) return;
    const key = location.key ?? "";
    if (key === lastLocationKey.current) return;
    lastLocationKey.current = key;

    if (state.quote) {
      setSessionId(null);
      setMessages([]);
      setQuote(state.quote);
      setTimeout(() => inputRef.current?.focus(), 100);
    } else if (state.prefill) {
      setSessionId(null);
      setMessages([]);
      setInput(state.prefill);
      setTimeout(() => inputRef.current?.focus(), 100);
    } else if (state.initialMessage) {
      setSessionId(null);
      setMessages([]);
      sendMessage(state.initialMessage, true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.key]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && !composingRef.current && !e.nativeEvent.isComposing && e.keyCode !== 229) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <div className="chat-page">
      <div className="chat-header">
        <div className="chat-header-left">
          <button
            className="btn btn-ghost btn-sm"
            onClick={() => setShowSessions(!showSessions)}
            title={t("chat.history")}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="3" y1="6" x2="21" y2="6" />
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="18" x2="21" y2="18" />
            </svg>
          </button>
          {messages.length > 0 && (
            <button className="btn btn-ghost btn-sm" onClick={startNewSession}>
              {t("chat.newChat")}
            </button>
          )}
        </div>
        {reflecting && (
          <span className="chat-reflecting">{t("chat.reflecting")}</span>
        )}
      </div>

      <div className="chat-body">
        {showSessions && (
          <div className="chat-sessions">
            <div className="chat-sessions-header">
              <span>{t("chat.conversations")}</span>
              <span className="chat-sessions-count">{sessions.length}</span>
            </div>
            <div className="chat-sessions-list">
              {sessions.length === 0 ? (
                <div className="chat-sessions-empty">{t("chat.noConversations")}</div>
              ) : (
                sessions.map((s) => (
                  <div
                    key={s.session_id}
                    className={`chat-session-item ${s.session_id === sessionId ? "active" : ""}`}
                    onClick={() => loadSession(s.session_id)}
                  >
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="chat-session-preview">
                        {s.preview || t("chat.emptyConversation")}
                      </div>
                      <div className="chat-session-meta">
                        <span>{formatSessionDate(s.updated_at)}</span>
                        <span>{s.message_count} {t("chat.msgs")}</span>
                      </div>
                    </div>
                    <button
                      className="chat-session-delete"
                      title={t("chat.delete")}
                      onClick={(e) => {
                        e.stopPropagation();
                        invoke("delete_chat_session", { sessionId: s.session_id }).then(() => {
                          if (s.session_id === sessionId) {
                            setSessionId(null);
                            setMessages([]);
                          }
                          loadSessions();
                        });
                      }}
                    >
                      ✕
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        <div className="chat-main">
          <div className="chat-messages" ref={scrollRef}>
            {messages.length === 0 && (
              <div className="chat-empty">
                <p className="chat-empty-title">{t("chat.emptyTitle")}</p>
                <p className="chat-empty-hint">
                  {t("chat.emptyHint1")}<br />
                  {t("chat.emptyHint2")}
                </p>
              </div>
            )}
            {messages.map((msg) => (
              <div key={msg.id} className={`chat-bubble ${msg.role}`}>
                <div className="chat-bubble-content">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                </div>
              </div>
            ))}
            {loading && (
              <div className="chat-bubble sage">
                <div className="chat-bubble-content chat-typing loading">
                  <span /><span /><span />
                </div>
              </div>
            )}
          </div>

          <div className="chat-input-area">
            {quote && (
              <div className="chat-quote-preview">
                <div className="chat-quote-text"><ReactMarkdown remarkPlugins={[remarkGfm]}>{quote}</ReactMarkdown></div>
                <button className="chat-quote-close" onClick={() => setQuote(null)}>✕</button>
              </div>
            )}
            <textarea
              ref={inputRef}
              className="chat-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              onCompositionStart={() => { composingRef.current = true; }}
              onCompositionEnd={() => { composingRef.current = false; }}
              placeholder={t("chat.placeholder")}
              rows={1}
              disabled={loading}
            />
            {loading ? (
              <button
                className="chat-send-btn chat-stop-btn"
                onClick={() => invoke("cancel_chat")}
                title={t("chat.stopGenerating")}
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                  <rect x="6" y="6" width="12" height="12" rx="2" />
                </svg>
              </button>
            ) : (
              <button
                className="chat-send-btn"
                onClick={() => sendMessage()}
                disabled={!input.trim()}
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="22" y1="2" x2="11" y2="13" />
                  <polygon points="22 2 15 22 11 13 2 9 22 2" />
                </svg>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export default Chat;
