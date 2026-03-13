import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Message {
  id: number;
  sender: string;
  channel: string;
  content: string | null;
  source: string;
  message_type: string;
  timestamp: string;
  created_at: string;
}

interface ChannelInfo {
  channel: string;
  source: string;
  count: number;
}

const SOURCE_COLORS: Record<string, string> = {
  teams: "#6264A7",
  email: "#0078D4",
  slack: "#4A154B",
};

function sourceIcon(source: string): string {
  switch (source) {
    case "teams": return "T";
    case "email": return "@";
    case "slack": return "#";
    default: return "?";
  }
}

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    if (isNaN(d.getTime())) return ts;
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffDays = Math.floor(diffMs / 86400000);
    if (diffDays === 0) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (diffDays === 1) return "Yesterday";
    if (diffDays < 7) return d.toLocaleDateString([], { weekday: "short" });
    return d.toLocaleDateString([], { month: "short", day: "numeric" });
  } catch {
    return ts;
  }
}

function MessageFlow() {
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [messages, setMessages] = useState<Message[]>([]);
  const [selectedChannel, setSelectedChannel] = useState<string | null>(null);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [aiInsight, setAiInsight] = useState<string | null>(null);
  const [aiLoading, setAiLoading] = useState(false);

  const initDone = useRef(false);

  const loadChannels = useCallback(async () => {
    try {
      const data = await invoke<ChannelInfo[]>("get_message_channels");
      setChannels(data);
      if (data.length > 0 && !initDone.current) {
        initDone.current = true;
        setSelectedChannel(data[0].channel);
        setSelectedSource(data[0].source);
      }
    } catch (e) {
      console.error("Failed to load channels:", e);
    }
  }, []);

  const loadMessages = useCallback(async () => {
    if (!selectedChannel && !selectedSource) {
      setMessages([]);
      setLoading(false);
      return;
    }
    setLoading(true);
    setAiInsight(null);
    try {
      const params: Record<string, unknown> = { limit: 100 };
      if (selectedChannel) params.channel = selectedChannel;
      else if (selectedSource) params.source = selectedSource;
      const data = await invoke<Message[]>("get_messages", params);
      setMessages(data);
    } catch (e) {
      console.error("Failed to load messages:", e);
    } finally {
      setLoading(false);
    }
  }, [selectedChannel, selectedSource]);

  useEffect(() => {
    loadChannels();
  }, [loadChannels]);

  useEffect(() => {
    loadMessages();
  }, [loadMessages]);

  // AI insight: summarize current messages
  const handleAiInsight = useCallback(async () => {
    if (messages.length === 0) return;
    setAiLoading(true);
    try {
      // Take the most recent 30 messages for context
      const recentMsgs = messages.slice(0, 30).map((m) =>
        `[${m.source}] ${m.sender}: ${m.content || "(no content)"}`,
      ).join("\n");
      const label = selectedChannel || selectedSource || "messages";
      const result = await invoke<string>("summarize_messages", {
        context: recentMsgs,
        label,
      });
      setAiInsight(result);
    } catch (e) {
      console.error("AI insight failed:", e);
      setAiInsight("Failed to generate insight.");
    } finally {
      setAiLoading(false);
    }
  }, [messages, selectedChannel, selectedSource]);

  const totalMessages = channels.reduce((sum, c) => sum + c.count, 0);
  const sources = [...new Set(channels.map((c) => c.source))];

  // Filter channels by selected source when no specific channel is selected
  const visibleChannels = selectedSource && !selectedChannel
    ? channels.filter((ch) => ch.source === selectedSource)
    : channels;

  const filteredMessages = searchQuery
    ? messages.filter(
        (m) =>
          m.content?.toLowerCase().includes(searchQuery.toLowerCase()) ||
          m.sender.toLowerCase().includes(searchQuery.toLowerCase()),
      )
    : messages;

  // Group messages by date
  const grouped = new Map<string, Message[]>();
  for (const msg of filteredMessages) {
    const dateKey = msg.timestamp.slice(0, 10);
    if (!grouped.has(dateKey)) grouped.set(dateKey, []);
    grouped.get(dateKey)!.push(msg);
  }

  return (
    <div style={{ display: "flex", height: "100%", gap: 0 }}>
      {/* Channel sidebar */}
      <div
        style={{
          width: 200,
          flexShrink: 0,
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div style={{ padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)" }}>
          <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
            {totalMessages} messages, {channels.length} channels
          </span>
        </div>

        {/* Source filter */}
        <div style={{ display: "flex", gap: 4, padding: "var(--spacing-sm) var(--spacing-md)", flexWrap: "wrap" }}>
          <button
            onClick={() => {
              setSelectedChannel(null);
              setSelectedSource(null);
              // Re-select first channel
              if (channels.length > 0) {
                setSelectedChannel(channels[0].channel);
                setSelectedSource(channels[0].source);
              }
            }}
            style={{
              fontSize: 10,
              padding: "2px 8px",
              borderRadius: 12,
              border: "1px solid var(--border)",
              background: selectedChannel ? "transparent" : "var(--surface-active)",
              color: "var(--text-secondary)",
              cursor: "pointer",
              fontWeight: 600,
            }}
          >
            ALL
          </button>
          {sources.map((src) => (
            <button
              key={src}
              onClick={() => {
                setSelectedChannel(null);
                setSelectedSource(src);
              }}
              style={{
                fontSize: 10,
                padding: "2px 8px",
                borderRadius: 12,
                border: "1px solid var(--border)",
                background: selectedSource === src && !selectedChannel ? SOURCE_COLORS[src] || "var(--accent)" : "transparent",
                color: selectedSource === src && !selectedChannel ? "#fff" : "var(--text-secondary)",
                cursor: "pointer",
                textTransform: "uppercase",
                fontWeight: 600,
                letterSpacing: "0.5px",
              }}
            >
              {src}
            </button>
          ))}
        </div>

        {/* Channel list — filtered by source */}
        <div style={{ flex: 1, overflowY: "auto" }}>
          {visibleChannels.map((ch) => (
            <button
              key={`${ch.channel}-${ch.source}`}
              onClick={() => {
                setSelectedChannel(ch.channel);
                setSelectedSource(ch.source);
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--spacing-sm)",
                width: "100%",
                padding: "6px var(--spacing-md)",
                border: "none",
                background: selectedChannel === ch.channel && selectedSource === ch.source ? "var(--surface-active)" : "transparent",
                cursor: "pointer",
                textAlign: "left",
                fontSize: 12,
                color: "var(--text)",
              }}
            >
              <span
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: 4,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: 10,
                  fontWeight: 700,
                  background: SOURCE_COLORS[ch.source] || "var(--text-tertiary)",
                  color: "#fff",
                  flexShrink: 0,
                }}
              >
                {sourceIcon(ch.source)}
              </span>
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {ch.channel}
              </span>
              <span style={{ fontSize: 10, color: "var(--text-tertiary)", flexShrink: 0 }}>{ch.count}</span>
            </button>
          ))}

          {visibleChannels.length === 0 && (
            <div style={{ padding: "var(--spacing-lg)", textAlign: "center", color: "var(--text-tertiary)", fontSize: 12 }}>
              {channels.length === 0
                ? "No channels yet. Install the Chrome extension to capture messages."
                : "No channels for this source."}
            </div>
          )}
        </div>
      </div>

      {/* Message list */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* Search + AI bar */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--spacing-sm)", padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)" }}>
          <input
            type="text"
            placeholder="Search messages..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            style={{
              flex: 1,
              padding: "6px 10px",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              background: "var(--bg)",
              color: "var(--text)",
              fontSize: 12,
              outline: "none",
            }}
          />
          <button
            onClick={handleAiInsight}
            disabled={aiLoading || messages.length === 0}
            style={{
              padding: "5px 10px",
              fontSize: 11,
              borderRadius: "var(--radius)",
              border: "1px solid var(--border)",
              background: "var(--surface)",
              color: "var(--text-secondary)",
              cursor: aiLoading || messages.length === 0 ? "not-allowed" : "pointer",
              opacity: aiLoading ? 0.6 : 1,
              whiteSpace: "nowrap",
              fontWeight: 500,
            }}
          >
            {aiLoading ? "Thinking..." : "AI Insight"}
          </button>
        </div>

        {/* AI Insight panel */}
        {aiInsight && (
          <div
            style={{
              margin: "var(--spacing-sm) var(--spacing-md)",
              padding: "var(--spacing-sm) var(--spacing-md)",
              background: "var(--accent-light)",
              borderRadius: "var(--radius)",
              border: "1px solid var(--accent)",
              fontSize: 12,
              lineHeight: 1.6,
              color: "var(--text)",
              position: "relative",
            }}
          >
            <button
              onClick={() => setAiInsight(null)}
              style={{
                position: "absolute",
                top: 4,
                right: 8,
                background: "none",
                border: "none",
                fontSize: 14,
                cursor: "pointer",
                color: "var(--text-tertiary)",
                padding: 0,
                lineHeight: 1,
              }}
            >
              x
            </button>
            <div style={{ fontSize: 10, fontWeight: 600, color: "var(--accent)", marginBottom: 4, textTransform: "uppercase", letterSpacing: "0.5px" }}>
              Sage Insight
            </div>
            <div style={{ whiteSpace: "pre-wrap" }}>{aiInsight}</div>
          </div>
        )}

        {/* Messages */}
        <div style={{ flex: 1, overflowY: "auto", padding: "var(--spacing-sm) 0" }}>
          {loading ? (
            <div style={{ textAlign: "center", padding: "var(--spacing-xl)", color: "var(--text-tertiary)" }}>
              Loading...
            </div>
          ) : filteredMessages.length === 0 ? (
            <div style={{ textAlign: "center", padding: "var(--spacing-xl)", color: "var(--text-tertiary)", fontSize: 13 }}>
              {searchQuery ? "No matching messages" : "No messages in this channel"}
            </div>
          ) : (
            [...grouped.entries()].map(([date, msgs]) => (
              <div key={date}>
                {/* Date divider */}
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: "var(--spacing-sm)",
                    padding: "var(--spacing-sm) var(--spacing-md)",
                    position: "sticky",
                    top: 0,
                    background: "var(--bg)",
                    zIndex: 1,
                  }}
                >
                  <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
                  <span style={{ fontSize: 10, color: "var(--text-tertiary)", fontWeight: 500, whiteSpace: "nowrap" }}>
                    {date}
                  </span>
                  <div style={{ flex: 1, height: 1, background: "var(--border)" }} />
                </div>

                {msgs.map((msg) => (
                  <div
                    key={msg.id}
                    style={{
                      display: "flex",
                      gap: "var(--spacing-sm)",
                      padding: "4px var(--spacing-md)",
                      transition: "background 0.15s",
                    }}
                    onMouseEnter={(e) => (e.currentTarget.style.background = "var(--surface-hover)")}
                    onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
                  >
                    {/* Avatar */}
                    <div
                      style={{
                        width: 28,
                        height: 28,
                        borderRadius: "50%",
                        background: SOURCE_COLORS[msg.source] ? `${SOURCE_COLORS[msg.source]}22` : "var(--accent-light)",
                        color: SOURCE_COLORS[msg.source] || "var(--accent)",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        fontSize: 12,
                        fontWeight: 600,
                        flexShrink: 0,
                        marginTop: 2,
                      }}
                    >
                      {msg.sender ? msg.sender.charAt(0).toUpperCase() : sourceIcon(msg.source)}
                    </div>

                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "baseline", gap: "var(--spacing-sm)" }}>
                        <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text)" }}>
                          {msg.sender || msg.source}
                        </span>
                        <span
                          onClick={() => {
                            setSelectedChannel(null);
                            setSelectedSource(msg.source);
                          }}
                          style={{
                            fontSize: 9,
                            padding: "1px 5px",
                            borderRadius: 4,
                            background: SOURCE_COLORS[msg.source] || "var(--text-tertiary)",
                            color: "#fff",
                            fontWeight: 600,
                            textTransform: "uppercase",
                            letterSpacing: "0.3px",
                            cursor: "pointer",
                          }}
                        >
                          {msg.source}
                        </span>
                        <span style={{ fontSize: 10, color: "var(--text-tertiary)" }}>{formatTime(msg.timestamp)}</span>
                      </div>
                      <div style={{ fontSize: 13, color: "var(--text)", lineHeight: 1.5, wordBreak: "break-word" }}>
                        {msg.content || <span style={{ color: "var(--text-tertiary)", fontStyle: "italic" }}>[no content]</span>}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

export default MessageFlow;
