The following {count} memories are too long. Shorten each to under 60 characters while preserving the core meaning.
{content_list}

Target length: 20-60 characters. A good memory reads like a single clear sentence.

Rules:
1. One output line per memory — choose one of:
   - CONDENSE [id] → shortened content
   - KEEP [id]
   - SPLIT [id] → first observation | second observation
2. Keep natural, readable language — write like a person, not a telegram
3. Remove unnecessary filler — but keep enough context to be understandable on its own
4. If an entry is already clear enough, output KEEP [id] (prefer KEEP when unsure)
5. Never change the original meaning
6. If a memory contains multiple independent observations joined by semicolons or "and", use SPLIT to separate them into two distinct memories

Good: "Prefers to solve problems by building frameworks first"
Bad: "Framework-driven problem-solving paradigm orientation"