/// 文本相似度工具：基于最长公共子序列（LCS）

/// 计算两段文本的相似度（0.0 ~ 1.0）
pub fn text_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let (m, n) = (a_chars.len(), b_chars.len());
    if m == 0 || n == 0 {
        return 0.0;
    }
    let max_len = m.max(n);
    // 长文本取头尾采样避免 O(n*m) 爆炸
    if max_len > 200 {
        let a_sample: Vec<char> = a_chars.iter().take(100)
            .chain(a_chars.iter().rev().take(50).collect::<Vec<_>>().into_iter().rev())
            .copied().collect();
        let b_sample: Vec<char> = b_chars.iter().take(100)
            .chain(b_chars.iter().rev().take(50).collect::<Vec<_>>().into_iter().rev())
            .copied().collect();
        let lcs = lcs_len(&a_sample, &b_sample);
        return lcs as f64 / a_sample.len().max(b_sample.len()) as f64;
    }
    let lcs = lcs_len(&a_chars, &b_chars);
    lcs as f64 / max_len as f64
}

/// 在列表中查找相似度 > threshold 的条目，返回其索引
pub fn find_similar(items: &[String], candidate: &str, threshold: f64) -> Option<usize> {
    for (i, item) in items.iter().enumerate() {
        if text_similarity(item, candidate) > threshold {
            return Some(i);
        }
    }
    None
}

fn lcs_len(a: &[char], b: &[char]) -> usize {
    let n = b.len();
    let mut prev = vec![0u16; n + 1];
    let mut curr = vec![0u16; n + 1];
    for &ac in a {
        for (j, &bc) in b.iter().enumerate() {
            curr[j + 1] = if ac == bc {
                prev[j] + 1
            } else {
                prev[j + 1].max(curr[j])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|x| *x = 0);
    }
    *prev.iter().max().unwrap_or(&0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical() {
        assert!(text_similarity("hello world", "hello world") > 0.99);
    }

    #[test]
    fn test_completely_different() {
        assert!(text_similarity("abc", "xyz") < 0.01);
    }

    #[test]
    fn test_similar_chinese() {
        let a = "不在对话中发送关怀提醒，如需提醒只用系统通知。";
        let b = "不在对话中插入关怀提醒；如需提醒，仅发系统通知。";
        assert!(text_similarity(a, b) > 0.6);
    }

    #[test]
    fn test_different_chinese() {
        let a = "不在对话中发送关怀提醒";
        let b = "每天早上九点发送邮件摘要";
        assert!(text_similarity(a, b) < 0.5);
    }

    #[test]
    fn test_find_similar() {
        let items = vec![
            "不发无效关怀提醒".to_string(),
            "每天九点发邮件".to_string(),
        ];
        assert_eq!(find_similar(&items, "不发关怀类提醒", 0.5), Some(0));
        assert_eq!(find_similar(&items, "完全不相关的内容", 0.5), None);
    }
}
