import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";

interface Message {
  id: number;
  role: "user" | "sage";
  content: string;
  session_id: string;
  created_at: string;
}

function Chat() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [reflecting, setReflecting] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

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
        }
      })
      .catch(console.error);
  }, []);

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
  };

  const sendMessage = async () => {
    const text = input.trim();
    if (!text || loading) return;

    const tempUserMsg: Message = {
      id: Date.now(),
      role: "user",
      content: text,
      session_id: sessionId || "",
      created_at: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, tempUserMsg]);
    setInput("");
    setLoading(true);

    try {
      const result = await invoke<{ response: string; session_id: string }>("chat", {
        message: text,
        sessionId: sessionId,
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
      const totalMsgs = messages.length + 2;
      if (totalMsgs > 0 && totalMsgs % 4 === 0) {
        triggerMemoryExtraction(result.session_id);
      }
    } catch (err) {
      const errStr = String(err);
      const isProviderError = errStr.includes("AI 服务") || errStr.includes("API");
      const errorMsg: Message = {
        id: Date.now() + 1,
        role: "sage",
        content: isProviderError
          ? "I'm not connected to an AI provider yet. Go to **Settings** to configure one, then come back and chat."
          : "Sorry, I'm unable to respond right now. Please try again later.",
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

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  return (
    <div className="chat-page">
      <div className="chat-header">
        {messages.length > 0 && (
          <button className="btn btn-ghost btn-sm" onClick={startNewSession}>
            New chat
          </button>
        )}
        {reflecting && (
          <span className="chat-reflecting">Sage is reflecting on this conversation...</span>
        )}
      </div>

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
                <ReactMarkdown>{msg.content}</ReactMarkdown>
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
          onClick={sendMessage}
          disabled={!input.trim() || loading}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="22" y1="2" x2="11" y2="13" />
            <polygon points="22 2 15 22 11 13 2 9 22 2" />
          </svg>
        </button>
      </div>
    </div>
  );
}

export default Chat;
