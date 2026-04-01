//! 文本截断工具函数（被 session_analyzer / task_intelligence 共用）

/// 截断字符串到 max_chars 个字符，返回原始字符串切片（无省略号）。
/// 保证 UTF-8 边界安全。
pub(crate) fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let byte_pos = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..byte_pos]
}

/// 截断字符串到 max_chars 个字符，超出时追加省略号 `…`。
pub(crate) fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let byte_pos = s
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}…", &s[..byte_pos])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_ascii() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_str_multibyte() {
        let s = "你好世界123";
        let result = truncate_str(s, 4);
        assert_eq!(result, "你好世界");
        assert!(result.chars().count() <= 4);
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(truncate_with_ellipsis("hello world", 5), "hello…");
        assert_eq!(truncate_with_ellipsis("hi", 10), "hi");
    }
}
