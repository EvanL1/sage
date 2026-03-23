以下是用户从其他 AI 助手（Claude/Gemini/ChatGPT）导出的记忆或个人信息：

{text}

请将其解析为结构化记忆条目。每条输出一行 JSON：
{"category": "...", "content": "...", "depth": "..."}

可用 category：identity, personality, values, behavior, thinking, emotion, growth, decision, pattern, preference, skill, relationship, goal

depth 层级（必填）：
- episodic：具体事件——必须包含日期、人名或具体动作。「3月20日和Bob开会」符合，「经常和人开会」不符合。
- semantic：行为规律——描述用户**倾向于做什么**。如「压力下倾向于砍范围」「沟通风格简洁直接」。不需要因果推理。
- procedural：判断逻辑——必须具备完整三要素：「遇到[情境]时，倾向于[做法]，因为[原因]」。如果无法填满三个部分，改用 semantic。这是最容易误标的层级。
- axiom：禁止标注——axiom 只通过演化生成，不能从原始信息直接提取

要求：
- 保留原始信息的核心内容，忠于原文
- 每条记忆简洁明了（1-2句话）
- 输出前验证每条：depth 为 "procedural" 时，检查是否有情境+做法+原因——若不完整，改为 "semantic"；depth 为 "axiom" 时，改为 "procedural" 或 "semantic"
- 只输出 JSON 行，不要其他内容（不要 markdown 代码块）