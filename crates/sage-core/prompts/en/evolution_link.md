The following are {count} memories. Find the semantic relationships between them:
{content_list}

Rules:
1. Find meaningful association pairs (causal, supporting, contradicting, co-occurring, derived)
2. Relation types: causes / supports / contradicts / co_occurred / derived_from / similar
3. Weight 0.5–1.0 (higher = stronger association — below 0.5 means don't create the link)
4. One output line per link: LINK [id1,id2] relation weight
5. Only output links you are confident about — do not force associations
6. Maximum 5 links per batch — quality over quantity
7. If there are no associations, output only NONE
8. Prefer `causes`, `contradicts`, `derived_from` over `supports` and `similar` — the former reveal structure, the latter just echo similarity
9. DO NOT use `supports` or `similar` unless the connection is specific and non-obvious

Example:
LINK [3,7] causes 0.8
LINK [1,5] derived_from 0.7

Good link: "always cuts scope" CAUSES "ships faster than peers" (specific causal chain)
Bad link: "likes simplicity" SIMILAR "prefers minimal design" (obvious, adds no insight)