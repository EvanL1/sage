use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use lettre::{
    message::header::ContentType, transport::smtp::authentication::Credentials, Message,
    SmtpTransport, Transport,
};
use mailparse::{parse_mail, MailHeaderMap};
use native_tls::TlsConnector;
use sage_types::{EmailMessage, Event, EventType, ImapSourceConfig, MessageSource};
use std::net::TcpStream;
use tracing::{info, warn};

use crate::channel::InputChannel;

// ── Host validation (SSRF prevention) ─────────────────────────────────────────

/// Reject IP addresses, localhost, and internal hostnames as mail hosts.
fn validate_mail_host(host: &str) -> Result<()> {
    if host.is_empty() {
        return Err(anyhow!("Mail host cannot be empty"));
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Err(anyhow!("IP addresses not allowed as mail hosts"));
    }
    let lower = host.to_lowercase();
    if lower == "localhost"
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower.ends_with(".lan")
        || lower == "127.0.0.1"
        || lower == "::1"
    {
        return Err(anyhow!("Internal hostnames not allowed as mail hosts"));
    }
    Ok(())
}

const MAX_EMAIL_BODY_BYTES: usize = 512 * 1024; // 512 KB per message

// ── Password encoding (NOT encryption — avoids cleartext in SQLite column) ────
// For real secret protection, use the OS keychain (macOS Keychain Services).

const ENCODE_KEY: u8 = 0x5A;

/// Encode password for storage. NOT encryption — just avoids plaintext in DB.
pub fn obfuscate(s: &str) -> String {
    let xored: Vec<u8> = s.bytes().map(|b| b ^ ENCODE_KEY).collect();
    STANDARD.encode(xored)
}

pub fn deobfuscate(s: &str) -> Result<String> {
    // If base64 decode fails, treat as raw password (e.g. during test_connection)
    match STANDARD.decode(s) {
        Ok(bytes) => {
            let plain: Vec<u8> = bytes.iter().map(|b| b ^ ENCODE_KEY).collect();
            String::from_utf8(plain).map_err(|e| anyhow!("utf8: {e}"))
        }
        Err(_) => {
            tracing::debug!("deobfuscate: base64 decode failed, treating as raw password");
            Ok(s.to_string())
        }
    }
}

// ── IMAP client ───────────────────────────────────────────────────────────────

pub struct ImapClient {
    pub(crate) config: ImapSourceConfig,
    pub(crate) source_id: i64,
}

/// XOAUTH2 authenticator for IMAP AUTHENTICATE command.
struct XOAuth2 {
    token: String,
}

impl imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        self.token.clone()
    }
}

impl ImapClient {
    pub fn new(config: ImapSourceConfig, source_id: i64) -> Self {
        Self { config, source_id }
    }

    /// Connect and authenticate (password or OAuth2), run `op`, then logout.
    fn with_session<F, R>(&self, op: F) -> Result<R>
    where
        F: FnOnce(&mut imap::Session<native_tls::TlsStream<TcpStream>>) -> Result<R>,
    {
        validate_mail_host(&self.config.imap_host)?;
        use std::net::ToSocketAddrs;
        let addr = format!("{}:{}", self.config.imap_host, self.config.imap_port)
            .to_socket_addrs()
            .map_err(|e| anyhow!("DNS resolution failed: {e}"))?
            .next()
            .ok_or_else(|| anyhow!("DNS resolution returned no addresses"))?;
        let tcp = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(20))
            .map_err(|e| anyhow!("IMAP TCP connect: {e}"))?;
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| anyhow!("set_read_timeout: {e}"))?;
        tcp.set_write_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| anyhow!("set_write_timeout: {e}"))?;
        let tls = TlsConnector::builder().build().map_err(|e| anyhow!("{e}"))?;
        let tls_stream = tls
            .connect(&self.config.imap_host, tcp)
            .map_err(|e| anyhow!("TLS handshake: {e}"))?;
        let client = imap::Client::new(tls_stream);

        let mut session = if self.config.auth_type == "oauth2" {
            // XOAUTH2: deobfuscate stored token, then authenticate
            let enc_token = self.config.oauth_access_token.as_deref()
                .ok_or_else(|| anyhow!("No OAuth2 access token"))?;
            let access_token = deobfuscate(enc_token)?;
            let sasl_token = format!("user={}\x01auth=Bearer {access_token}\x01\x01", self.config.email);
            let auth = XOAuth2 { token: STANDARD.encode(sasl_token.as_bytes()) };
            client
                .authenticate("XOAUTH2", &auth)
                .map_err(|(e, _)| anyhow!("IMAP XOAUTH2 auth: {e}"))?
        } else {
            // Password auth
            let password = deobfuscate(&self.config.password_enc)?;
            client
                .login(&self.config.username, &password)
                .map_err(|(e, _)| anyhow!("IMAP login: {e}"))?
        };

        let result = op(&mut session);
        let _ = session.logout();
        result
    }

    /// Fetch the N most-recent emails from `folder`.
    pub fn fetch_emails(&self, folder: &str, limit: usize) -> Result<Vec<EmailMessage>> {
        let source_id = self.source_id;
        let folder = folder.to_string();
        self.with_session(|session| do_fetch(session, source_id, &folder, limit, None))
    }

    /// Fetch unread emails from INBOX.
    pub fn fetch_unread(&self) -> Result<Vec<EmailMessage>> {
        let source_id = self.source_id;
        self.with_session(|session| do_fetch(session, source_id, "INBOX", 50, Some("UNSEEN")))
    }
}

fn do_fetch<S: std::io::Read + std::io::Write>(
    session: &mut imap::Session<S>,
    source_id: i64,
    folder: &str,
    limit: usize,
    search: Option<&str>,
) -> Result<Vec<EmailMessage>> {
    session
        .select(folder)
        .map_err(|e| anyhow!("SELECT {folder}: {e}"))?;

    let query = search.unwrap_or("ALL");
    let uids = session
        .search(query)
        .map_err(|e| anyhow!("SEARCH {query}: {e}"))?;

    if uids.is_empty() {
        return Ok(Vec::new());
    }

    let mut uids_vec: Vec<u32> = uids.into_iter().collect();
    uids_vec.sort_unstable();
    let take_from = uids_vec.len().saturating_sub(limit);
    let uid_set = uids_vec[take_from..]
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let fetches = session
        .uid_fetch(&uid_set, "(RFC822 FLAGS)")
        .map_err(|e| anyhow!("UID FETCH: {e}"))?;

    let mut result = Vec::new();
    for fetch in fetches.iter() {
        let uid = match fetch.uid {
            Some(u) => u.to_string(),
            None => continue, // skip messages without UID
        };
        let is_read = fetch
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen));
        let raw = match fetch.body() {
            Some(b) => b,
            None => continue,
        };
        if let Some(mut msg) = parse_raw_email(source_id, &uid, raw) {
            msg.is_read = is_read;
            msg.folder = folder.to_string(); // use actual folder, not hardcoded INBOX
            result.push(msg);
        }
    }

    Ok(result)
}

// ── Raw email parsing ─────────────────────────────────────────────────────────

fn parse_raw_email(source_id: i64, uid: &str, raw: &[u8]) -> Option<EmailMessage> {
    // Cap body size to prevent resource exhaustion from large emails
    let truncated = if raw.len() > MAX_EMAIL_BODY_BYTES {
        tracing::warn!("Email uid={uid} body too large ({} bytes), truncating", raw.len());
        &raw[..MAX_EMAIL_BODY_BYTES]
    } else {
        raw
    };
    let parsed = parse_mail(truncated).ok()?;
    let headers = &parsed.headers;

    let subject = headers.get_first_value("Subject").unwrap_or_default();
    let from_addr = headers.get_first_value("From").unwrap_or_default();
    let to_addr = headers.get_first_value("To").unwrap_or_default();
    let date = headers.get_first_value("Date").unwrap_or_default();

    let (body_text, body_html) = extract_body_parts(&parsed);

    Some(EmailMessage {
        id: 0,
        source_id,
        uid: uid.to_string(),
        folder: "INBOX".to_string(),
        from_addr,
        to_addr,
        subject,
        body_text,
        body_html,
        is_read: false,
        date,
        fetched_at: chrono::Local::now().to_rfc3339(),
    })
}

fn extract_body_parts(parsed: &mailparse::ParsedMail) -> (String, Option<String>) {
    if parsed.subparts.is_empty() {
        let ct = parsed
            .get_headers()
            .get_first_value("Content-Type")
            .unwrap_or_default()
            .to_lowercase();
        let body = parsed.get_body().unwrap_or_default();
        if ct.contains("text/html") {
            return (strip_html(&body), Some(body));
        }
        return (body, None);
    }

    let mut text_plain = None::<String>;
    let mut text_html = None::<String>;

    for part in &parsed.subparts {
        let ct = part
            .get_headers()
            .get_first_value("Content-Type")
            .unwrap_or_default()
            .to_lowercase();
        let body = part.get_body().unwrap_or_default();
        if ct.contains("text/plain") && text_plain.is_none() {
            text_plain = Some(body);
        } else if ct.contains("text/html") && text_html.is_none() {
            text_html = Some(body);
        } else if !part.subparts.is_empty() {
            let (p, h) = extract_body_parts(part);
            if text_plain.is_none() && !p.is_empty() {
                text_plain = Some(p);
            }
            if text_html.is_none() {
                text_html = h;
            }
        }
    }

    let body_text = text_plain
        .or_else(|| text_html.as_deref().map(strip_html))
        .unwrap_or_default();
    (body_text, text_html)
}

// ── SMTP sending ──────────────────────────────────────────────────────────────

pub fn send_email_sync(config: &ImapSourceConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    let password = deobfuscate(&config.password_enc)?;
    if password.is_empty() && config.auth_type == "oauth2" {
        return Err(anyhow!(
            "SMTP send via OAuth2 not yet supported. Use the web client to send replies."
        ));
    }
    let email = Message::builder()
        .from(config.email.parse().context("Invalid from address")?)
        .to(to.parse().context("Invalid to address")?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .context("Failed to build email")?;
    let creds = Credentials::new(config.username.clone(), password);
    let mailer = SmtpTransport::relay(&config.smtp_host)
        .context("SMTP relay creation failed")?
        .credentials(creds)
        .port(config.smtp_port)
        .build();
    mailer.send(&email).context("SMTP send failed")?;
    Ok(())
}

pub async fn send_email(config: &ImapSourceConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    let config = config.clone();
    let to = to.to_string();
    let subject = subject.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || send_email_sync(&config, &to, &subject, &body))
        .await
        .context("spawn_blocking failed")?
}

// ── EmailChannel ──────────────────────────────────────────────────────────────

pub struct EmailChannel {
    sources: Vec<MessageSource>,
}

impl EmailChannel {
    pub fn new(sources: Vec<MessageSource>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl InputChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn poll(&self) -> Result<Vec<Event>> {
        let mut all_events = Vec::new();

        for source in &self.sources {
            if !source.enabled || source.source_type != "imap" {
                continue;
            }
            let config: ImapSourceConfig = match serde_json::from_str(&source.config) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Source {} config parse error: {e}", source.id);
                    continue;
                }
            };
            let client = ImapClient::new(config, source.id);
            let source_id = source.id;

            let msgs = match tokio::task::spawn_blocking(move || client.fetch_unread()).await {
                Ok(Ok(m)) => m,
                Ok(Err(e)) => {
                    warn!("IMAP fetch source {source_id}: {e}");
                    continue;
                }
                Err(e) => {
                    warn!("spawn_blocking source {source_id}: {e}");
                    continue;
                }
            };

            let events: Vec<Event> = msgs.iter().filter_map(build_event_from_message).collect();
            if !events.is_empty() {
                info!(
                    "Email poll source {}: {} unread emails",
                    source_id,
                    events.len()
                );
            }
            all_events.extend(events);
        }

        Ok(all_events)
    }
}

// ── scan_recent_emails ────────────────────────────────────────────────────────

pub async fn scan_recent_emails(sources: &[MessageSource], _hours: u32) -> Result<String> {
    let mut lines = Vec::new();

    for source in sources {
        if !source.enabled || source.source_type != "imap" {
            continue;
        }
        let config: ImapSourceConfig = match serde_json::from_str(&source.config) {
            Ok(c) => c,
            Err(e) => {
                warn!("Source {} config parse error: {e}", source.id);
                continue;
            }
        };
        let client = ImapClient::new(config, source.id);
        let msgs = match tokio::task::spawn_blocking(move || client.fetch_emails("INBOX", 30)).await
        {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                warn!("scan_recent_emails fetch: {e}");
                continue;
            }
            Err(e) => {
                warn!("spawn_blocking: {e}");
                continue;
            }
        };

        for msg in msgs {
            if msg.subject.is_empty() {
                continue;
            }
            let tag = if msg.is_read { "[已读]" } else { "[未读]" };
            let clean = strip_html(&msg.body_text);
            let preview: String = clean.chars().take(150).collect();
            let suffix = if clean.chars().count() > 150 { "..." } else { "" };
            if preview.is_empty() {
                lines.push(format!("- {tag} **{}** — {}", msg.subject, msg.from_addr));
            } else {
                lines.push(format!(
                    "- {tag} **{}** — {}\n  > {preview}{suffix}",
                    msg.subject, msg.from_addr
                ));
            }
        }
    }

    if lines.is_empty() {
        return Ok(String::new());
    }

    Ok(format!("共 {} 封邮件：\n{}", lines.len(), lines.join("\n")))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_event_from_message(msg: &EmailMessage) -> Option<Event> {
    build_event(&msg.subject, &msg.from_addr, msg.body_text.clone(), &msg.date)
}

fn build_event(subject: &str, from: &str, body: String, date: &str) -> Option<Event> {
    if subject.is_empty() {
        return None;
    }
    let clean_body = strip_html(&body);
    let preview: String = clean_body.chars().take(500).collect();
    let suffix = if clean_body.chars().count() > 500 { "..." } else { "" };
    let body_text = if preview.is_empty() {
        format!("From: {from}")
    } else {
        format!("From: {from}\n\n{preview}{suffix}")
    };

    Some(Event {
        source: "email".into(),
        event_type: EventType::NewEmail,
        title: subject.to_string(),
        body: body_text,
        metadata: [
            ("from".into(), from.to_string()),
            ("priority".into(), "normal".into()),
            ("date".into(), date.to_string()),
            ("direction".into(), "received".into()),
        ]
        .into_iter()
        .collect(),
        timestamp: chrono::Local::now(),
    })
}

/// Remove all `<open_tag>...</close_tag>` blocks from a string (case-insensitive).
fn remove_tag_blocks(input: &str, open_tag: &str, close_tag: &str) -> String {
    let mut s = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let mut pos = 0;
    while pos < input.len() {
        if let Some(start_offset) = lower[pos..].find(open_tag) {
            s.push_str(&input[pos..pos + start_offset]);
            let after_open = pos + start_offset;
            if let Some(end_offset) = lower[after_open..].find(close_tag) {
                let close_start = after_open + end_offset;
                if let Some(gt) = input[close_start..].find('>') {
                    pos = close_start + gt + 1;
                } else {
                    pos = input.len();
                }
            } else {
                pos = input.len();
            }
        } else {
            s.push_str(&input[pos..]);
            break;
        }
    }
    s
}

/// Public wrapper for strip_html — used by outlook.rs
pub fn strip_html_public(input: &str) -> String {
    strip_html(input)
}

/// Strip HTML tags, style/script blocks, and entities from text.
fn strip_html(input: &str) -> String {
    // 1. Remove <style>...</style> blocks
    let mut s = remove_tag_blocks(input, "<style", "</style");
    // 1b. Remove <script>...</script> blocks
    s = remove_tag_blocks(&s, "<script", "</script");

    // 2. Remove all <...> tags
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // 3. Decode basic HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"");

    // 4. Collapse excess blank lines
    let mut cleaned = String::with_capacity(result.len());
    let mut blank_count = 0u32;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                cleaned.push('\n');
            }
        } else {
            blank_count = 0;
            if !cleaned.is_empty() && !cleaned.ends_with('\n') {
                cleaned.push('\n');
            }
            cleaned.push_str(trimmed);
        }
    }

    cleaned.trim().to_string()
}

/// Determine if an email event should be escalated to urgent.
pub fn should_upgrade_to_urgent(
    event: &Event,
    vip_domains: &[String],
    urgent_keywords: &[String],
) -> bool {
    if event
        .metadata
        .get("priority")
        .map(|p| p == "high")
        .unwrap_or(false)
    {
        return true;
    }
    let from = event.metadata.get("from").map(|s| s.as_str()).unwrap_or("");
    for domain in vip_domains {
        if from.contains(domain.as_str()) {
            return true;
        }
    }
    let title_lower = event.title.to_lowercase();
    for kw in urgent_keywords {
        if title_lower.contains(&kw.to_lowercase()) {
            return true;
        }
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- build_event ---

    #[test]
    fn test_build_event_basic() {
        let event = build_event(
            "Test Subject",
            "Alice <alice@example.com>",
            "Hello body".into(),
            "2026-03-21T10:00:00+08:00",
        )
        .unwrap();
        assert_eq!(event.title, "Test Subject");
        assert_eq!(
            event.metadata.get("from").unwrap(),
            "Alice <alice@example.com>"
        );
        assert!(event.body.contains("Hello body"));
    }

    #[test]
    fn test_build_event_no_subject_returns_none() {
        assert!(build_event("", "x@x.com", String::new(), "").is_none());
    }

    // --- strip_html ---

    #[test]
    fn test_strip_html_basic_tags() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn test_strip_html_style_block() {
        let input = "Before<style type=\"text/css\">.foo{color:red}</style>After";
        assert_eq!(strip_html(input), "BeforeAfter");
    }

    #[test]
    fn test_strip_html_entities() {
        assert_eq!(strip_html("A &amp; B &lt; C"), "A & B < C");
    }

    #[test]
    fn test_strip_html_collapses_blank_lines() {
        let input = "Line 1\n\n\n\nLine 2";
        let result = strip_html(input);
        assert_eq!(result, "Line 1\nLine 2");
    }

    #[test]
    fn test_strip_html_empty_input() {
        assert_eq!(strip_html(""), "");
    }

    // --- should_upgrade_to_urgent ---

    #[test]
    fn test_should_upgrade_to_urgent_by_priority() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
            title: "Normal subject".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "someone@random.com".into()),
                ("priority".into(), "high".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(&event, &[], &[]));
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_domain() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
            title: "Regular update".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "sam@example.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(
            &event,
            &["example.com".into()],
            &[]
        ));
    }

    #[test]
    fn test_should_upgrade_to_urgent_by_keyword() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
            title: "URGENT: Server down".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "ops@random.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(should_upgrade_to_urgent(&event, &[], &["urgent".into()]));
    }

    #[test]
    fn test_should_not_upgrade_normal_email() {
        let event = Event {
            source: "email".into(),
            event_type: EventType::NewEmail,
            title: "Weekly newsletter".into(),
            body: String::new(),
            metadata: [
                ("from".into(), "news@random.com".into()),
                ("priority".into(), "normal".into()),
            ]
            .into_iter()
            .collect(),
            timestamp: chrono::Local::now(),
        };
        assert!(!should_upgrade_to_urgent(
            &event,
            &["example.com".into()],
            &["urgent".into()]
        ));
    }

    // --- obfuscate / deobfuscate ---

    #[test]
    fn test_obfuscate_deobfuscate_roundtrip() {
        let original = "my_s3cr3t_p@ssw0rd!";
        let enc = obfuscate(original);
        let dec = deobfuscate(&enc).unwrap();
        assert_eq!(dec, original);
    }

    #[test]
    fn test_obfuscate_empty_string() {
        let enc = obfuscate("");
        assert_eq!(enc, "");
        let dec = deobfuscate(&enc).unwrap();
        assert_eq!(dec, "");
    }

    #[test]
    fn test_deobfuscate_invalid_base64_fallback() {
        // Invalid base64 should fallback to returning raw input
        let result = deobfuscate("raw_password_123").unwrap();
        assert_eq!(result, "raw_password_123");
    }

    // --- parse_raw_email ---

    #[test]
    fn test_parse_raw_email_plain_text() {
        let raw = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: Hello\r\n\
            Date: Mon, 21 Mar 2026 10:00:00 +0800\r\n\
            Content-Type: text/plain; charset=utf-8\r\n\
            \r\n\
            Hello, this is a plain text email.\r\n";
        let msg = parse_raw_email(1, "42", raw).unwrap();
        assert_eq!(msg.subject, "Hello");
        assert_eq!(msg.from_addr, "alice@example.com");
        assert_eq!(msg.source_id, 1);
        assert_eq!(msg.uid, "42");
        assert!(msg.body_text.contains("plain text email"));
        assert!(msg.body_html.is_none());
    }

    #[test]
    fn test_parse_raw_email_html_only() {
        let raw = b"From: sender@example.com\r\n\
            To: recv@example.com\r\n\
            Subject: HTML Email\r\n\
            Date: Mon, 21 Mar 2026 12:00:00 +0800\r\n\
            Content-Type: text/html; charset=utf-8\r\n\
            \r\n\
            <html><body><p>Hello <b>World</b></p></body></html>\r\n";
        let msg = parse_raw_email(2, "99", raw).unwrap();
        assert_eq!(msg.subject, "HTML Email");
        assert!(msg.body_text.contains("Hello"));
        assert!(msg.body_html.is_some());
        assert!(!msg.body_text.contains("<b>"));
    }

    #[test]
    fn test_parse_raw_email_multipart() {
        let raw = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: Multipart Test\r\n\
            Date: Mon, 21 Mar 2026 14:00:00 +0800\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/alternative; boundary=\"boundary42\"\r\n\
            \r\n\
            --boundary42\r\n\
            Content-Type: text/plain; charset=utf-8\r\n\
            \r\n\
            Plain text part.\r\n\
            --boundary42\r\n\
            Content-Type: text/html; charset=utf-8\r\n\
            \r\n\
            <p>HTML part</p>\r\n\
            --boundary42--\r\n";
        let msg = parse_raw_email(3, "7", raw).unwrap();
        assert_eq!(msg.subject, "Multipart Test");
        assert!(msg.body_text.contains("Plain text part"));
        assert!(msg.body_html.is_some());
        assert!(msg
            .body_html
            .as_deref()
            .unwrap()
            .contains("<p>HTML part</p>"));
    }
}
