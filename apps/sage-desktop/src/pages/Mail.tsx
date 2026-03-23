import { useEffect, useState, useCallback } from "react";
import { Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import DOMPurify from "dompurify";
import type { MessageSource, EmailMessage } from "../types";

const FOLDERS = ["INBOX", "Sent", "Drafts", "Spam", "Trash"];

function formatDate(dateStr: string): string {
  try {
    const d = new Date(dateStr);
    if (isNaN(d.getTime())) return dateStr;
    const now = new Date();
    const diffDays = Math.floor((now.getTime() - d.getTime()) / 86400000);
    if (diffDays === 0) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (diffDays === 1) return "Yesterday";
    if (diffDays < 7) return d.toLocaleDateString([], { weekday: "short" });
    return d.toLocaleDateString([], { month: "short", day: "numeric" });
  } catch {
    return dateStr;
  }
}

function EmailListItem({ email, selected, onClick }: { email: EmailMessage; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        display: "flex", flexDirection: "column", gap: 3,
        width: "100%", padding: "10px var(--spacing-md)", border: "none",
        borderBottom: "1px solid var(--border)",
        background: selected ? "var(--surface-active)" : "transparent",
        cursor: "pointer", textAlign: "left",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 4 }}>
        <span style={{
          fontSize: 12, fontWeight: email.is_read ? 400 : 700,
          color: "var(--text)", flex: 1,
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        }}>
          {email.subject || "(no subject)"}
        </span>
        <span style={{ fontSize: 10, color: "var(--text-tertiary)", flexShrink: 0 }}>
          {formatDate(email.date)}
        </span>
      </div>
      <span style={{ fontSize: 11, color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {email.from_addr}
      </span>
    </button>
  );
}

function EmailDetail({ email, onMarkRead, onDismiss }: { email: EmailMessage; onMarkRead: () => void; onDismiss: () => void }) {
  const [summary, setSummary] = useState<string | null>(null);
  const [summaryLoading, setSummaryLoading] = useState(false);
  const [replyOpen, setReplyOpen] = useState(false);
  const [replyText, setReplyText] = useState("");
  const [sendingReply, setSendingReply] = useState(false);
  const [smartReplyLoading, setSmartReplyLoading] = useState(false);

  useEffect(() => {
    setSummary(null);
    setReplyOpen(false);
    setReplyText("");
  }, [email.id]);

  const handleAiSummary = useCallback(async () => {
    setSummaryLoading(true);
    try {
      const result = await invoke<string>("summarize_email", { emailId: email.id });
      setSummary(result);
    } catch (err) {
      setSummary("Failed to summarize: " + String(err));
    } finally {
      setSummaryLoading(false);
    }
  }, [email.id]);

  const handleSmartReply = useCallback(async () => {
    setSmartReplyLoading(true);
    setReplyOpen(true);
    try {
      const result = await invoke<string>("smart_reply", { emailId: email.id });
      setReplyText(result);
    } catch (err) {
      setReplyText("Failed to generate reply: " + String(err));
    } finally {
      setSmartReplyLoading(false);
    }
  }, [email.id]);

  const handleSendReply = useCallback(async () => {
    if (!replyText.trim()) return;
    setSendingReply(true);
    try {
      await invoke("send_email", {
        sourceId: email.source_id,
        to: email.from_addr,
        subject: "Re: " + email.subject,
        body: replyText,
      });
      setReplyOpen(false);
      setReplyText("");
    } catch (err) {
      alert("Failed to send: " + String(err));
    } finally {
      setSendingReply(false);
    }
  }, [email, replyText]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", overflow: "hidden" }}>
      {/* Header */}
      <div style={{ padding: "var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text)", marginBottom: 8 }}>
          {email.subject || "(no subject)"}
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 12, color: "var(--text-secondary)" }}>
          <div><span style={{ color: "var(--text-tertiary)", width: 36, display: "inline-block" }}>From</span>{email.from_addr}</div>
          <div><span style={{ color: "var(--text-tertiary)", width: 36, display: "inline-block" }}>To</span>{email.to_addr}</div>
          <div><span style={{ color: "var(--text-tertiary)", width: 36, display: "inline-block" }}>Date</span>{email.date}</div>
        </div>
      </div>

      {/* Action buttons */}
      <div style={{ display: "flex", gap: "var(--spacing-sm)", padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0, flexWrap: "wrap" }}>
        {!email.is_read && (
          <button className="btn btn-secondary btn-sm" onClick={onMarkRead}>
            Mark Read
          </button>
        )}
        <button className="btn btn-secondary btn-sm" onClick={() => setReplyOpen((v) => !v)}>
          Reply
        </button>
        <button
          className="btn btn-secondary btn-sm"
          onClick={handleAiSummary}
          disabled={summaryLoading}
        >
          {summaryLoading ? "Summarizing..." : "AI Summary"}
        </button>
        <button
          className="btn btn-secondary btn-sm"
          onClick={handleSmartReply}
          disabled={smartReplyLoading}
        >
          {smartReplyLoading ? "Generating..." : "Smart Reply"}
        </button>
        <button
          className="btn btn-ghost btn-sm"
          style={{ color: "var(--error-text)", marginLeft: "auto" }}
          onClick={onDismiss}
        >
          Delete
        </button>
      </div>

      {/* AI Summary banner */}
      {summary && (
        <div style={{
          margin: "var(--spacing-sm) var(--spacing-md)",
          padding: "var(--spacing-sm) var(--spacing-md)",
          background: "var(--accent-light)", borderRadius: "var(--radius)",
          border: "1px solid var(--accent)", fontSize: 12, lineHeight: 1.6,
          color: "var(--text)", position: "relative", flexShrink: 0,
        }}>
          <button
            onClick={() => setSummary(null)}
            style={{ position: "absolute", top: 4, right: 8, background: "none", border: "none", fontSize: 14, cursor: "pointer", color: "var(--text-tertiary)", padding: 0, lineHeight: 1 }}
          >
            x
          </button>
          <div style={{ fontSize: 10, fontWeight: 600, color: "var(--accent)", marginBottom: 4, textTransform: "uppercase", letterSpacing: "0.5px" }}>
            AI Summary
          </div>
          <div style={{ whiteSpace: "pre-wrap", paddingRight: 16 }}>{summary}</div>
        </div>
      )}

      {/* Email body */}
      <div style={{ flex: 1, overflowY: "auto", padding: "var(--spacing-md)" }}>
        {email.body_html ? (
          <div
            style={{ fontSize: 13, lineHeight: 1.6, color: "var(--text)" }}
            dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(email.body_html, {
              ALLOWED_TAGS: ["p","br","span","div","h1","h2","h3","h4","h5","h6",
                "strong","em","b","i","u","s","blockquote","pre","code",
                "ul","ol","li","table","thead","tbody","tr","td","th","a","hr"],
              ALLOWED_ATTR: ["href","style","class","colspan","rowspan","width","height"],
            }) }}
          />
        ) : (
          <pre style={{ fontSize: 13, lineHeight: 1.6, color: "var(--text)", whiteSpace: "pre-wrap", fontFamily: "inherit", margin: 0 }}>
            {email.body_text || "(no content)"}
          </pre>
        )}
      </div>

      {/* Reply compose */}
      {replyOpen && (
        <div style={{ flexShrink: 0, padding: "var(--spacing-md)", borderTop: "1px solid var(--border)" }}>
          <textarea
            value={replyText}
            onChange={(e) => setReplyText(e.target.value)}
            placeholder="Write your reply..."
            style={{
              width: "100%", minHeight: 100, padding: "var(--spacing-sm)",
              border: "1px solid var(--border)", borderRadius: "var(--radius)",
              background: "var(--bg)", color: "var(--text)", fontSize: 13,
              fontFamily: "inherit", resize: "vertical", boxSizing: "border-box",
            }}
          />
          <div style={{ display: "flex", gap: "var(--spacing-sm)", marginTop: "var(--spacing-sm)" }}>
            <button className="btn btn-primary btn-sm" onClick={handleSendReply} disabled={sendingReply || !replyText.trim()}>
              {sendingReply ? "Sending..." : "Send"}
            </button>
            <button className="btn btn-secondary btn-sm" onClick={() => setReplyOpen(false)}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Mail() {
  const [sources, setSources] = useState<MessageSource[]>([]);
  const [selectedSourceId, setSelectedSourceId] = useState<number | null>(null);
  const [folder, setFolder] = useState("INBOX");
  const [emails, setEmails] = useState<EmailMessage[]>([]);
  const [selectedEmail, setSelectedEmail] = useState<EmailMessage | null>(null);
  const [loading, setLoading] = useState(false);
  const [sourcesLoading, setSourcesLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    invoke<MessageSource[]>("get_message_sources")
      .then((s) => {
        setSources(s);
        if (s.length > 0) setSelectedSourceId(s[0].id);
      })
      .catch(() => {})
      .finally(() => setSourcesLoading(false));
  }, []);

  // Load cached emails when source/folder changes
  useEffect(() => {
    if (!selectedSourceId) return;
    setLoading(true);
    setSelectedEmail(null);
    invoke<EmailMessage[]>("get_cached_emails", { sourceId: selectedSourceId, folder, limit: 50 })
      .then(setEmails)
      .catch(() => setEmails([]))
      .finally(() => setLoading(false));
  }, [selectedSourceId, folder]);

  const handleRefresh = useCallback(async () => {
    if (!selectedSourceId) return;
    setRefreshing(true);
    try {
      const currentSource = sources.find((s) => s.id === selectedSourceId);
      const isOutlook = currentSource?.source_type === "outlook";
      if (isOutlook) {
        const fetched = await invoke<EmailMessage[]>("fetch_outlook_emails", { sourceId: selectedSourceId, limit: 50 });
        setEmails(fetched);
      } else {
        // Refresh OAuth2 token if needed before IMAP fetch
        await invoke("ensure_oauth_token", { sourceId: selectedSourceId }).catch(() => {});
        const fetched = await invoke<EmailMessage[]>("fetch_emails", { sourceId: selectedSourceId, folder, limit: 50 });
        setEmails(fetched);
      }
    } catch (err) {
      console.error("Fetch failed:", err);
    } finally {
      setRefreshing(false);
    }
  }, [selectedSourceId, folder, sources]);

  const handleMarkRead = useCallback(async () => {
    if (!selectedEmail) return;
    try {
      await invoke("mark_email_read", { emailId: selectedEmail.id });
      setEmails((prev) => prev.map((e) => e.id === selectedEmail.id ? { ...e, is_read: true } : e));
      setSelectedEmail((prev) => prev ? { ...prev, is_read: true } : prev);
    } catch (err) {
      console.error("Mark read failed:", err);
    }
  }, [selectedEmail]);

  if (sourcesLoading) {
    return (
      <div style={{ display: "flex", height: "100%", alignItems: "center", justifyContent: "center" }}>
        <span style={{ color: "var(--text-secondary)" }}>Loading...</span>
      </div>
    );
  }

  if (sources.length === 0) {
    return (
      <div style={{ display: "flex", height: "100%", alignItems: "center", justifyContent: "center" }}>
        <div style={{ textAlign: "center" }}>
          <div style={{ fontSize: 14, color: "var(--text-secondary)", marginBottom: 8 }}>No email sources configured</div>
          <Link to="/settings" style={{ fontSize: 13, color: "var(--accent)", textDecoration: "none" }}>
            Configure email in Settings
          </Link>
        </div>
      </div>
    );
  }

  const unreadCount = emails.filter((e) => !e.is_read).length;

  return (
    <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>
      {/* Left panel */}
      <div style={{ width: 240, flexShrink: 0, borderRight: "1px solid var(--border)", display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* Source selector */}
        <div style={{ padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
          <select
            className="form-select"
            value={selectedSourceId ?? ""}
            onChange={(e) => setSelectedSourceId(Number(e.target.value))}
            style={{ width: "100%", fontSize: 12 }}
          >
            {sources.map((s) => (
              <option key={s.id} value={s.id}>{s.label}</option>
            ))}
          </select>
        </div>

        {/* Folder selector */}
        <div style={{ padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0, display: "flex", flexDirection: "column", gap: 2 }}>
          {FOLDERS.map((f) => (
            <button
              key={f}
              onClick={() => setFolder(f)}
              style={{
                display: "flex", alignItems: "center", justifyContent: "space-between",
                padding: "5px 8px", border: "none", borderRadius: "var(--radius)",
                background: folder === f ? "var(--surface-active)" : "transparent",
                color: folder === f ? "var(--text)" : "var(--text-secondary)",
                cursor: "pointer", textAlign: "left", fontSize: 12, fontWeight: folder === f ? 600 : 400,
              }}
            >
              <span>{f}</span>
              {f === "INBOX" && unreadCount > 0 && (
                <span style={{
                  fontSize: 10, fontWeight: 700, background: "var(--accent)", color: "#fff",
                  borderRadius: 999, padding: "1px 6px", minWidth: 16, textAlign: "center",
                }}>
                  {unreadCount}
                </span>
              )}
            </button>
          ))}
        </div>

        {/* Refresh button */}
        <div style={{ padding: "var(--spacing-sm) var(--spacing-md)", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
          <button
            className="btn btn-secondary btn-sm"
            onClick={handleRefresh}
            disabled={refreshing}
            style={{ width: "100%" }}
          >
            {refreshing ? "Refreshing..." : "Refresh"}
          </button>
        </div>

        {/* Email list */}
        <div style={{ flex: 1, overflowY: "auto" }}>
          {loading ? (
            <div style={{ padding: "var(--spacing-lg)", textAlign: "center", color: "var(--text-tertiary)", fontSize: 12 }}>
              Loading...
            </div>
          ) : emails.length === 0 ? (
            <div style={{ padding: "var(--spacing-lg)", textAlign: "center", color: "var(--text-tertiary)", fontSize: 12 }}>
              Click Refresh to fetch
            </div>
          ) : (
            emails.map((email) => (
              <EmailListItem
                key={email.id}
                email={email}
                selected={selectedEmail?.id === email.id}
                onClick={() => setSelectedEmail(email)}
              />
            ))
          )}
        </div>
      </div>

      {/* Right panel */}
      <div style={{ flex: 1, overflow: "hidden" }}>
        {selectedEmail ? (
          <EmailDetail email={selectedEmail} onMarkRead={handleMarkRead} onDismiss={async () => {
            await invoke("dismiss_email", { emailId: selectedEmail.id }).catch(() => {});
            setEmails(prev => prev.filter(e => e.id !== selectedEmail.id));
            setSelectedEmail(null);
          }} />
        ) : (
          <div style={{ display: "flex", height: "100%", alignItems: "center", justifyContent: "center" }}>
            <span style={{ fontSize: 13, color: "var(--text-tertiary)" }}>Select an email to read</span>
          </div>
        )}
      </div>
    </div>
  );
}

export default Mail;
