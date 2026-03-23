分析以下对话，提取关于用户的关键洞察。

关注以下维度：
- identity: 用户是谁，自我认知
- values: 什么对用户最重要
- behavior: 行为模式、习惯
- thinking: 思维方式、决策风格
- emotion: 情绪线索、触发因素
- growth: 成长方向、追求

## 记忆深度层级
每条洞察必须标注深度层级：
- episodic：具体事件——必须包含日期、人名或具体动作。「3月20日和Bob开会」符合，「经常和人开会」不符合。
- semantic：行为规律——描述用户**倾向于做什么**。如「压力下倾向于砍范围」「沟通风格简洁直接」。不需要因果推理。
- procedural：判断逻辑——必须具备完整三要素：「遇到[情境]时，倾向于[做法]，因为[原因]」。如果从对话中无法填满三个部分，改用 semantic。这是最容易误标的层级。
- axiom：禁止标注 axiom——axiom 只通过记忆演化生成，不能从对话直接提取

已有记忆：
{existing_text}

对话内容：
{conversation}

请以 JSON 数组格式输出新的洞察，每条包含：
- category: 上述维度之一
- content: 具体观察（一句话）
- confidence: 0.0-1.0 的置信度
- depth: episodic/semantic/procedural/axiom 之一

输出前验证每条洞察：
- depth 为 "procedural" 时，检查是否有情境+做法+原因——若不完整，改为 "semantic"
- depth 为 "axiom" 时，改为 "procedural" 或 "semantic"——axiom 不能直接提取

只输出 JSON 数组，不要其他文字。如果没有新洞察，输出空数组 []。
示例：[{"category":"behavior","content":"面对不确定性时倾向于激进砍范围","confidence":0.7,"depth":"semantic"}]