## 新写入的信息
{new_content}

## 现有记忆
{items_text}

任务：判断新信息是否推翻、矛盾或纠正了以上任何条目。

规则：
- 只标记事实层面的矛盾，不做主观判断
- 不矛盾的记忆不要列出

输出格式（严格遵守，每条一行）：
REVISE id: one-line reason why the original conclusion no longer holds

如果没有任何矛盾，只输出：NONE
不要输出其他内容。