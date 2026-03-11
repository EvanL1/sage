import { useState, useRef, useEffect, useCallback } from "react";
import { useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { formatDate } from "../utils/time";

interface Message {
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
  const location = useLocation();
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [reflecting, setReflecting] = useState(false);
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [showSessions, setShowSessions] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const initialMessageHandled = useRef(false);
  const msgCountRef = useRef(0);

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
    invoke<Message[]>("get_chat_history", { limit: 50 })
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
      const history = await invoke<Message[]>("get_chat_history", { sessionId: sid });
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
    const text = (overrideText ?? input).trim();
    if (!text || loading) return;

    const sid = forceNewSession ? null : sessionId;

    const tempUserMsg: Message = {
      id: Date.now(),
      role: "user",
      content: text,
      session_id: sid || "",
      created_at: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, tempUserMsg]);
    setInput("");
    setLoading(true);

    try {
      const result = await invoke<{ response: string; session_id: string }>("chat", {
        message: text,
        sessionId: sid,
      });

      if (!sessionId) {
        setSessionId(result.session_id);
      }

      const sageMsg: Message = {
        id: Date.now() + 1,
        role: "sage",
        content: result.response,
        session_id: result.session_id,
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, sageMsg]);

      // Trigger memory extraction every 4 messages (2 rounds)
      msgCountRef.current += 2;
      if (msgCountRef.current % 4 === 0) {
        triggerMemoryExtraction(result.session_id);
      }

      loadSessions();
    } catch (err) {
      const errStr = String(err);
      const isProviderError = errStr.includes("AI 服务") || errStr.includes("API");
      const errorContent = isProviderError
        ? "I'm not connected to an AI provider yet. Go to **Settings** to configure one, then come back and chat."
        : `Something went wrong: ${errStr}`;
      const errorMsg: Message = {
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

  // 从 Dashboard/History 跳转过来时，强制开新 session 发送
  useEffect(() => {
    const state = location.state as { initialMessage?: string } | null;
    if (state?.initialMessage && !initialMessageHandled.current) {
      initialMessageHandled.current = true;
      setSessionId(null);
      setMessages([]);
      sendMessage(state.initialMessage, true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.state]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
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
            title="Chat history"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="3" y1="6" x2="21" y2="6" />
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="18" x2="21" y2="18" />
            </svg>
          </button>
          {messages.length > 0 && (
            <button className="btn btn-ghost btn-sm" onClick={startNewSession}>
              New chat
            </button>
          )}
        </div>
        {reflecting && (
          <span className="chat-reflecting">Sage is reflecting on this conversation...</span>
        )}
      </div>

      <div className="chat-body">
        {showSessions && (
          <div className="chat-sessions">
            <div className="chat-sessions-header">
              <span>Conversations</span>
              <span className="chat-sessions-count">{sessions.length}</span>
            </div>
            <div className="chat-sessions-list">
              {sessions.length === 0 ? (
                <div className="chat-sessions-empty">No conversations yet</div>
              ) : (
                sessions.map((s) => (
                  <button
                    key={s.session_id}
                    className={`chat-session-item ${s.session_id === sessionId ? "active" : ""}`}
                    onClick={() => loadSession(s.session_id)}
                  >
                    <div className="chat-session-preview">
                      {s.preview || "Empty conversation"}
                    </div>
                    <div className="chat-session-meta">
                      <span>{formatSessionDate(s.updated_at)}</span>
                      <span>{s.message_count} msgs</span>
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>
        )}

        <div className="chat-main">
          <div className="chat-messages" ref={scrollRef}>
            {messages.length === 0 && (
              <div className="chat-empty">
                <p className="chat-empty-title">Chat with Sage</p>
                <p className="chat-empty-hint">
                  Every conversation helps me understand you better.<br />
                  Ask me anything — work decisions, self-reflection, or just talk.
                </p>
              </div>
            )}
            {messages.map((msg) => (
              <div key={msg.id} className={`chat-bubble ${msg.role}`}>
                <div className="chat-bubble-content">
                  {msg.role === "sage" ? (
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{msg.content}</ReactMarkdown>
                  ) : (
                    msg.content
                  )}
                </div>
              </div>
            ))}
            {loading && (
              <div className="chat-bubble sage">
                <div className="chat-bubble-content chat-typing">
                  <span /><span /><span />
                </div>
              </div>
            )}
          </div>

          <div className="chat-input-area">
            <textarea
              ref={inputRef}
              className="chat-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Say something..."
              rows={1}
              disabled={loading}
            />
            <button
              className="chat-send-btn"
              onClick={() => sendMessage()}
              disabled={!input.trim() || loading}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export default Chat;
