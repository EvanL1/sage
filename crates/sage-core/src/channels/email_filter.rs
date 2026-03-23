//! Email importance classifier — distinguishes human mail from automated noise.
//!
//! Classification: "human" (person wrote it), "useful" (automated but worth reading),
//! "noise" (notifications, receipts, alerts — skip).

/// Classify an email's importance based on sender and subject.
/// Returns "human", "useful", or "noise".
pub fn classify(from: &str, subject: &str, body: &str) -> &'static str {
    let from_lower = from.to_lowercase();
    let subject_lower = subject.to_lowercase();

    // ── Useful exceptions (check BEFORE noise patterns) ──
    // GitHub PR reviews, mentions, assignments are worth reading
    if from_lower.contains("github.com")
        && (subject_lower.contains("review")
            || subject_lower.contains("mentioned")
            || subject_lower.contains("assigned"))
    {
        return "useful";
    }

    // ── Definite noise: noreply / system senders ──
    if is_noreply_sender(&from_lower) && !has_substantive_content(body) {
        return "noise";
    }

    // ── Meeting auto-replies (noise — no useful content) ──
    let auto_reply_prefixes = [
        "已接受:", "已拒绝:", "已暂定:",
        "accepted:", "declined:", "tentative:",
    ];
    for prefix in &auto_reply_prefixes {
        if subject_lower.starts_with(prefix) {
            return "noise";
        }
    }

    // ── Known noise patterns by sender domain ──
    let noise_domains = [
        "statuspage.io",
        "github.com",
        "orders.apple.com",
        "apple.com",
        "accounts.google.com",
        "npmjs.com",
    ];
    for domain in &noise_domains {
        if from_lower.contains(domain) {
            return "noise";
        }
    }

    // ── Known noise by subject prefix (catches empty-sender automated mail) ──
    let noise_prefixes = [
        "[github]",          // GitHub app notifications
        "re: [github]",
    ];
    for prefix in &noise_prefixes {
        if subject_lower.starts_with(prefix) && !subject_lower.contains("review") && !subject_lower.contains("mentioned") {
            return "noise";
        }
    }

    // ── Known noise by body content (catches empty-sender automated mail) ──
    let body_lower = body.to_lowercase();
    let noise_body_signals = [
        "you're receiving this email because",
        "you are receiving this because",
        "this is an automated message",
        "do not reply to this email",
        "unsubscribe",
        "manage your subscription",
        "you received this email because you are subscribed",
    ];
    let body_noise_count = noise_body_signals.iter().filter(|s| body_lower.contains(*s)).count();
    if body_noise_count >= 2 {
        return "noise"; // multiple automated signals = definitely noise
    }

    // ── Known noise patterns by subject ──
    let noise_subjects = [
        "incident",
        "security alert",
        "电子保单",
        "电子账单",
        "电子收据",
        "电子发票",
        "订单",
        "取货信息",
        "personal access token",
        "confirm your subscription",
        "password",
        "verify your",
        "安全提醒",
        "passkey",
        "requesting updated permissions",
        "requesting additional access",
        "successfully published",
    ];
    for pattern in &noise_subjects {
        if subject_lower.contains(pattern) {
            return "noise";
        }
    }

    // ── Automated but potentially useful ──
    if is_noreply_sender(&from_lower) {
        return "useful"; // noreply but not caught by noise patterns — might be useful
    }

    // ── Default: human-sent ──
    "human"
}

fn is_noreply_sender(from: &str) -> bool {
    from.contains("noreply")
        || from.contains("no-reply")
        || from.contains("no_reply")
        || from.contains("do_not_reply")
        || from.contains("donotreply")
        || from.contains("notifications@")
        || from.contains("mailer-daemon")
        || from.contains("postmaster@")
}

/// Check if the email body has enough substance to be worth reading.
/// Very short auto-generated content (< 50 chars of actual text) is noise.
fn has_substantive_content(body: &str) -> bool {
    let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    clean.len() > 100
}

/// 去除邮件正文中的签名档和引用链，保留真正的正文内容。
/// 不截断正文，只去除重复/无信息量的尾部。
pub fn strip_signature_and_quotes(body: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        // 签名分隔符
        if trimmed == "--" || trimmed == "-- " || trimmed == "---" {
            break;
        }
        // Outlook 风格引用链起始
        if trimmed.starts_with("发件人:") || trimmed.starts_with("From:") {
            // 检查是否是邮件头引用（下一行通常是 日期:/Date:）
            break;
        }
        // 分隔线（连续下划线或等号 ≥10）
        if trimmed.len() >= 10
            && (trimmed.chars().all(|c| c == '_') || trimmed.chars().all(|c| c == '='))
        {
            break;
        }
        // Outlook 签名块起始（名字 + 职位 + 电话号码模式）
        if trimmed.starts_with("+86 ") || trimmed.starts_with("+1 ") {
            break;
        }
        lines.push(line);
    }
    // 去尾部空行
    while lines.last().map_or(false, |l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_email() {
        assert_eq!(classify("alice@company.com", "Q1 Budget Review", "Please review the attached budget proposal..."), "human");
    }

    #[test]
    fn test_noise_statuspage() {
        assert_eq!(classify("noreply@statuspage.io", "Claude Incident - Down", "We are investigating..."), "noise");
    }

    #[test]
    fn test_noise_github_notification() {
        assert_eq!(classify("noreply@github.com", "Your token is expiring", ""), "noise");
    }

    #[test]
    fn test_useful_github_review() {
        assert_eq!(classify("noreply@github.com", "PR review requested", "Please review this PR"), "useful");
    }

    #[test]
    fn test_noise_order() {
        assert_eq!(classify("order@orders.apple.com", "Apple 订单确认", "Your order has been placed"), "noise");
    }

    #[test]
    fn test_noise_security_alert() {
        assert_eq!(classify("no-reply@accounts.google.com", "Security alert for your account", ""), "noise");
    }

    #[test]
    fn test_noise_receipt() {
        assert_eq!(classify("billing@company.com", "电子发票已开具", ""), "noise");
    }

    #[test]
    fn test_human_meeting_notes() {
        assert_eq!(classify("evan.liu@voltageenergy.com", "2026.03.23 EMS周会会议纪要", "各位同事，以下是本周EMS团队周会会议纪要..."), "human");
    }

    #[test]
    fn test_useful_noreply_with_content() {
        assert_eq!(classify("noreply@jira.company.com", "Bug report assigned to you", "A long detailed bug description that is definitely worth reading because it contains important information about the system failure"), "useful");
    }

    #[test]
    fn test_noise_meeting_accepted() {
        assert_eq!(classify("alice@company.com", "已接受: Weekly Sync", ""), "noise");
        assert_eq!(classify("alice@company.com", "Accepted: Weekly Sync", ""), "noise");
        assert_eq!(classify("alice@company.com", "已拒绝: 1:1 Meeting", ""), "noise");
    }

    // ── strip_signature_and_quotes tests ──

    #[test]
    fn test_strip_outlook_signature() {
        let body = "请审批预算方案。\n\n王相 - Shawn Wang\n光储系统工程师\n+86 198 8301 8724\n_______________";
        let stripped = strip_signature_and_quotes(body);
        assert_eq!(stripped, "请审批预算方案。\n\n王相 - Shawn Wang\n光储系统工程师");
    }

    #[test]
    fn test_strip_reply_chain() {
        let body = "好的，收到。\n\n发件人: Alice\n日期: 2026-03-20\n主题: 预算审批\n\n请审批。";
        let stripped = strip_signature_and_quotes(body);
        assert_eq!(stripped, "好的，收到。");
    }

    #[test]
    fn test_strip_dash_separator() {
        let body = "正文内容\n--\nSent from my iPhone";
        let stripped = strip_signature_and_quotes(body);
        assert_eq!(stripped, "正文内容");
    }

    #[test]
    fn test_no_strip_needed() {
        let body = "这是一封正常的邮件。\n没有签名档。";
        let stripped = strip_signature_and_quotes(body);
        assert_eq!(stripped, body);
    }
}
