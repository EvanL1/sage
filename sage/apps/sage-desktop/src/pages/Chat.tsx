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

  // 滚动到底部
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  // 聚焦输入框
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // 加载历史消息
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

      // 每 4 条消息（2 轮对话）触发一次记忆提取
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
          ? "我还没有连接到思考能力。请到**设置**页面配置 AI 服务，然后回来找我聊天。"
          : "抱歉，我暂时无法回应。请稍后再试。",
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
            新对话
          </button>
        )}
        {reflecting && (
          <span className="chat-reflecting">Sage 正在反思这段对话...</span>
        )}
      </div>

      <div className="chat-messages" ref={scrollRef}>
        {messages.length === 0 && (
          <div className="chat-empty">
            <p className="chat-empty-title">和 Sage 聊聊</p>
            <p className="chat-empty-hint">
              每一次对话都会让我更了解你。<br />
              问我任何事 — 工作决策、自我探索、或只是聊聊天。
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
          placeholder="说点什么..."
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
