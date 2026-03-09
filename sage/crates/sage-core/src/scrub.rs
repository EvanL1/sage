//! 凭证脱敏模块
//!
//! 在将用户输入或 LLM 响应写入日志、数据库或传输前，
//! 自动替换 API Key、Bearer Token、密码等敏感凭证为 `[REDACTED]`。

use std::sync::LazyLock;

/// 静态编译的凭证检测正则表达式集合（仅初始化一次）
static PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    vec![
        // OpenAI / Anthropic 风格 API Key：sk- 开头，后跟至少 20 位字母数字
        regex::Regex::new(r"sk-[a-zA-Z0-9_-]{20,}").unwrap(),
        // HTTP Authorization Bearer Token：Bearer 空格后跟至少 20 位
        regex::Regex::new(r"Bearer\s+[a-zA-Z0-9._-]{20,}").unwrap(),
        // 显式的 api_key / api-key / apikey 赋值，支持 = 或 : 分隔
        // 注意：原始字符串中不用 \" 转义，改用字符类 [a-zA-Z0-9_-]
        regex::Regex::new(r"(?i)api[_-]?key[=:]\s*[a-zA-Z0-9_-]{16,}").unwrap(),
        // password 赋值（明文密码），至少 8 位非空白字符
        regex::Regex::new(r"(?i)password[=:]\s*\S{8,}").unwrap(),
    ]
});

/// 对输入字符串做凭证脱敏，返回安全副本。
///
/// 匹配到的凭证内容将被替换为 `[REDACTED]`，其余文本保持不变。
/// 对不含敏感信息的普通文本无影响。
pub fn scrub_credentials(input: &str) -> String {
    let mut result = input.to_string();
    for pat in PATTERNS.iter() {
        result = pat.replace_all(&result, "[REDACTED]").to_string();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_api_key() {
        // Anthropic / OpenAI 风格 API Key 应被替换
        let input = "使用密钥 sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890 调用 API";
        let result = scrub_credentials(input);
        assert!(
            !result.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890"),
            "API Key 应被脱敏"
        );
        assert!(result.contains("[REDACTED]"), "应包含 [REDACTED] 占位符");
    }

    #[test]
    fn test_scrub_bearer_token() {
        // Bearer Token 应被替换
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload";
        let result = scrub_credentials(input);
        assert!(
            !result.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
            "Bearer Token 应被脱敏"
        );
        assert!(result.contains("[REDACTED]"), "应包含 [REDACTED] 占位符");
    }

    #[test]
    fn test_scrub_password() {
        // password= 赋值应被替换
        let input = "config: password=MySecret123";
        let result = scrub_credentials(input);
        assert!(!result.contains("MySecret123"), "明文密码应被脱敏");
        assert!(result.contains("[REDACTED]"), "应包含 [REDACTED] 占位符");
    }

    #[test]
    fn test_no_false_positive() {
        // 普通文本不应被误替换
        let input = "今天天气不错，团队会议在下午3点。这是一个正常的句子，不含任何凭证。";
        let result = scrub_credentials(input);
        assert_eq!(result, input, "普通文本不应被修改");
    }

    #[test]
    fn test_scrub_multiple() {
        // 同一字符串中多个凭证都应被替换
        let input = concat!(
            "api_key=abcdef1234567890abcd ",
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.xxxxx ",
            "password=VerySecret!"
        );
        let result = scrub_credentials(input);
        assert!(
            !result.contains("abcdef1234567890abcd"),
            "api_key 值应被脱敏"
        );
        assert!(
            !result.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
            "Bearer token 应被脱敏"
        );
        assert!(!result.contains("VerySecret!"), "密码应被脱敏");
        // 应出现三处 [REDACTED]
        let redacted_count = result.matches("[REDACTED]").count();
        assert!(
            redacted_count >= 3,
            "应有至少 3 处 [REDACTED]，实际: {}\n结果: {}",
            redacted_count,
            result
        );
    }

    #[test]
    fn test_scrub_api_key_with_colon() {
        // api_key: 赋值格式
        let input = r#"{"api_key": "sk-1234567890abcdefghij"}"#;
        let result = scrub_credentials(input);
        assert!(!result.contains("sk-1234567890abcdefghij"), "sk- 格式 API Key 应被脱敏");
    }
}
