use chrono::{Datelike, Duration, Local};

/// 将记忆内容中的相对时间引用替换为绝对日期。
/// 写入时调用一次，确保存储的记忆不会因时间推移而变得语义模糊。
pub fn normalize_time_refs(content: &str) -> String {
    let today = Local::now().date_naive();
    let replacements = [
        // 中文
        ("今天", today),
        ("今日", today),
        ("昨天", today - Duration::days(1)),
        ("昨日", today - Duration::days(1)),
        ("前天", today - Duration::days(2)),
        ("明天", today + Duration::days(1)),
        ("明日", today + Duration::days(1)),
        ("后天", today + Duration::days(2)),
        // 英文
        ("today", today),
        ("yesterday", today - Duration::days(1)),
        ("tomorrow", today + Duration::days(1)),
    ];

    let week_replacements = [
        // 中文周
        ("本周", week_range(today, 0)),
        ("这周", week_range(today, 0)),
        ("上周", week_range(today, -1)),
        ("下周", week_range(today, 1)),
        ("this week", week_range(today, 0)),
        ("last week", week_range(today, -1)),
        ("next week", week_range(today, 1)),
    ];

    let mut result = content.to_string();

    for (word, date) in &replacements {
        if result.contains(word) {
            result = result.replace(word, &date.format("%Y-%m-%d").to_string());
        }
    }

    for (word, range) in &week_replacements {
        if result.contains(word) {
            result = result.replace(word, range);
        }
    }

    result
}

/// 计算某周的 "MM-DD~MM-DD" 范围字符串
fn week_range(today: chrono::NaiveDate, offset_weeks: i64) -> String {
    let weekday = today.weekday().num_days_from_monday() as i64;
    let monday = today - Duration::days(weekday) + Duration::weeks(offset_weeks);
    let sunday = monday + Duration::days(6);
    format!("{}~{}", monday.format("%m-%d"), sunday.format("%m-%d"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_today_zh() {
        let result = normalize_time_refs("今天开了仿真培训");
        let expected_date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(result, format!("{expected_date}开了仿真培训"));
    }

    #[test]
    fn test_normalize_yesterday_zh() {
        let result = normalize_time_refs("昨天完成了代码审查");
        let expected = (Local::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert!(result.contains(&expected));
    }

    #[test]
    fn test_normalize_tomorrow_en() {
        let result = normalize_time_refs("meeting tomorrow with team");
        let expected = (Local::now().date_naive() + Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, format!("meeting {expected} with team"));
    }

    #[test]
    fn test_normalize_no_change() {
        let input = "Alex 偏好直接沟通风格";
        assert_eq!(normalize_time_refs(input), input);
    }

    #[test]
    fn test_normalize_week_zh() {
        let result = normalize_time_refs("本周重点是发布");
        assert!(!result.contains("本周"));
        // Should contain MM-DD~MM-DD pattern
        assert!(result.contains('~'));
    }

    #[test]
    fn test_normalize_multiple() {
        let result = normalize_time_refs("今天和昨天都在写代码");
        assert!(!result.contains("今天"));
        assert!(!result.contains("昨天"));
    }
}
