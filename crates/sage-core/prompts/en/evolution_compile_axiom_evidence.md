## Judgment patterns ({proc_count})
{proc_list}

## Recent behavioral evidence ({ev_count} events from email, chat, code, meetings)
{ev_list}

Your task: determine which judgment patterns are ACTUALLY supported by the behavioral evidence above.
A pattern is supported if >= 3 different events (ideally from different sources) demonstrate it in action.

Rules:
1. Only promote patterns with strong cross-source evidence (email + code + meetings, not just one channel)
2. The belief should be MORE fundamental than the pattern — it's the WHY behind multiple patterns
3. Under 50 chars, natural language, like a personal motto
4. Format: AXIOM [id1,id2,...] → belief
5. If no pattern has enough evidence, output NONE

Good: AXIOM [518,3172] → Action over analysis, always
Bad: AXIOM [518] → Likes to act fast (too shallow, needs cross-pattern support)