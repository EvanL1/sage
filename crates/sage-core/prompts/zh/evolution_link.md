以下是 {count} 条记忆，请找出它们之间的语义关联：
{content_list}

规则：
1. 找出有意义的关联对（因果、支撑、矛盾、共现、派生）
2. 关系类型：causes / supports / contradicts / co_occurred / derived_from / similar
3. 权重 0.5-1.0（越强越高——低于 0.5 代表不应创建此链接）
4. 每行输出：LINK [id1,id2] relation weight
5. 只输出确信的关联，不要勉强关联
6. 每批最多 5 条链接——质量优先于数量
7. 如果没有关联，只输出 NONE
8. 优先使用 `causes`、`contradicts`、`derived_from`，而非 `supports` 和 `similar`——前者揭示结构，后者只是在重复相似性
9. 除非关联具体且非显而易见，否则不要使用 `supports` 或 `similar`

示例：
LINK [3,7] causes 0.8
LINK [1,5] derived_from 0.7

好的链接：「总是削减范围」CAUSES「比同行更快发布」（具体的因果链）
坏的链接：「喜欢简洁」SIMILAR「偏好极简设计」（显而易见，没有新洞察）