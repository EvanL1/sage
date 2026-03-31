/// 文本相似度工具：LCS + bigram Jaccard 取最大值
/// LCS 捕获顺序相似，Jaccard 捕获关键词重叠（对中文尤其重要）

/// 计算两段文本的相似度（0.0 ~ 1.0）
pub fn text_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let (m, n) = (a_chars.len(), b_chars.len());
    if m == 0 || n == 0 {
        return 0.0;
    }

    // LCS score
    let max_len = m.max(n);
    let lcs_score = if max_len > 200 {
        let a_sample: Vec<char> = a_chars.iter().take(100)
            .chain(a_chars.iter().rev().take(50).collect::<Vec<_>>().into_iter().rev())
            .copied().collect();
        let b_sample: Vec<char> = b_chars.iter().take(100)
            .chain(b_chars.iter().rev().take(50).collect::<Vec<_>>().into_iter().rev())
            .copied().collect();
        let lcs = lcs_len(&a_sample, &b_sample);
        lcs as f64 / a_sample.len().max(b_sample.len()) as f64
    } else {
        let lcs = lcs_len(&a_chars, &b_chars);
        lcs as f64 / max_len as f64
    };

    // Bigram Jaccard — 用字符 bigram 模拟中文"词"，捕获关键词重叠
    let jaccard_score = bigram_jaccard(&a_chars, &b_chars);

    // Keyword overlap — 对中文语义去重最有效（提取实体词，overlap coefficient）
    let kw_score = keyword_overlap(a, b);

    lcs_score.max(jaccard_score).max(kw_score)
}

/// 字符 bigram Jaccard 相似度
fn bigram_jaccard(a: &[char], b: &[char]) -> f64 {
    use std::collections::HashSet;
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let a_set: HashSet<(char, char)> = a.windows(2).map(|w| (w[0], w[1])).collect();
    let b_set: HashSet<(char, char)> = b.windows(2).map(|w| (w[0], w[1])).collect();
    let intersection = a_set.intersection(&b_set).count();
    let union = a_set.union(&b_set).count();
    if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
}

/// 关键词重叠度：提取有意义的词元（英文单词、中文 2-gram 去停用词），计算 Jaccard
/// 对"同一件事不同表述"的中文任务去重效果远好于字符级 bigram
fn keyword_overlap(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    fn extract_tokens(s: &str) -> HashSet<String> {
        let mut tokens = HashSet::new();
        let mut ascii_buf = String::new();
        let chars: Vec<char> = s.chars().collect();
        for &c in &chars {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                ascii_buf.push(c.to_ascii_lowercase());
            } else {
                if ascii_buf.len() >= 2 {
                    tokens.insert(ascii_buf.clone());
                }
                ascii_buf.clear();
            }
        }
        if ascii_buf.len() >= 2 { tokens.insert(ascii_buf); }
        // 中文：连续汉字取 2-gram，跳过停用字
        let stop = ['的', '了', '在', '是', '和', '与', '或', '将', '至', '为', '也',
                     '有', '到', '从', '对', '被', '把', '这', '那', '个', '们', '中'];
        let cjk: Vec<char> = chars.iter()
            .filter(|c| c.is_alphabetic() && !c.is_ascii_alphabetic() && !stop.contains(c))
            .copied().collect();
        for w in cjk.windows(2) {
            tokens.insert(format!("{}{}", w[0], w[1]));
        }
        tokens
    }
    let ta = extract_tokens(a);
    let tb = extract_tokens(b);
    if ta.is_empty() || tb.is_empty() { return 0.0; }
    let inter = ta.intersection(&tb).count();
    let smaller = ta.len().min(tb.len());
    // 用较小集合做分母（overlap coefficient），对"短任务 vs 长任务"更敏感
    inter as f64 / smaller as f64
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
    fn test_semantic_dedup_chinese() {
        // 同一件事不同表述 — LCS 低但 bigram Jaccard 应该 catch
        let a = "OA 流程超时，立即登录 OA 审批推进";
        let b = "OA 流程超时处理：登录 OA 系统完成审批";
        assert!(text_similarity(a, b) > 0.5, "score={}", text_similarity(a, b));
    }

    #[test]
    fn test_keyword_overlap() {
        // 关键词重叠但语序不同
        let a = "voltage-modbus 调试结论补写";
        let b = "确认 voltage-modbus 调试是否已结案，补一行结论";
        assert!(text_similarity(a, b) > 0.4, "score={}", text_similarity(a, b));
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

    #[test]
    fn test_keyword_overlap_chinese() {
        // 关键词提取（汇报/完成）能比纯字符 bigram 更好地捕获语义重叠
        let a = "完成给Li的汇报PPT（约3页）";
        let b = "将 PLUSE CEO 汇报材料准备截止提前至 04-01 汇报前完成";
        let score = keyword_overlap(a, b);
        // 共享 "汇报" + "完成"，overlap coefficient ≈ 0.28
        assert!(score > 0.2, "keyword_overlap={score:.3}, expected > 0.2");
    }

    #[test]
    fn test_keyword_overlap_english() {
        let a = "finish the PPT report for Li";
        let b = "prepare PLUSE CEO report materials before deadline";
        let score = keyword_overlap(a, b);
        // 共享 "report"，overlap coefficient ≈ 0.17
        assert!(score > 0.1, "keyword_overlap={score:.3}, expected > 0.1");
    }

    #[test]
    fn test_keyword_overlap_boosts_text_similarity() {
        let a = "完成给Li的汇报PPT（约3页）";
        let b = "将 PLUSE CEO 汇报材料准备截止提前至 04-01 汇报前完成";
        let old_lcs = {
            let ac: Vec<char> = a.chars().collect();
            let bc: Vec<char> = b.chars().collect();
            bigram_jaccard(&ac, &bc).max({
                let lcs = lcs_len(&ac, &bc);
                lcs as f64 / ac.len().max(bc.len()) as f64
            })
        };
        let new_score = text_similarity(a, b);
        assert!(new_score > old_lcs, "keyword_overlap should boost: {new_score:.3} vs {old_lcs:.3}");
    }
}
