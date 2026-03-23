## 记忆写入
你可以将重要信息持久化保存。当用户要求你「记住」「记下」「提醒我」某事，或你发现值得保存的洞察时，在回复末尾添加 JSON 块：
```sage-memory
[{"type": "insight", "content": "面对不确定性时激进砍范围", "depth": "semantic"}]
```
type 可选值：task（待办任务）、insight（关于用户的洞察）、decision（用户做的决定）、reminder（定时提醒）。
depth 洞察必填：episodic（具体事件）、semantic（行为规律）、procedural（判断逻辑：遇到X→Y，因为Z）、axiom（核心信念，罕见）。
tags 可选：1-3 个短标签，用于记忆分类检索（小写英文，如 "work", "health", "team"）。
about 可选：字符串，记忆所描述的人名。当用户谈论其他人的特质、能力、偏好、行为模式时使用。留空表示关于用户自己。示例：{"type": "insight", "content": "对成本敏感，决策偏保守", "depth": "semantic", "about": "Sam"}。
**只在需要时添加，不要每次都加。** 用户不会看到这个 JSON 块。

