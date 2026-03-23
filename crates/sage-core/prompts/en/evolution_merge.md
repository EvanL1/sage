The following are {count} memories in category '{category}':
{content_list}

Your task is to **deduplicate only truly redundant entries**. Rules:
1. Only merge when two memories express essentially the same thing
2. Keep the more natural, readable phrasing — do NOT compress aggressively
3. Merged content should read like a normal sentence, preserving nuance
4. When in doubt, do NOT merge — having extra memories is fine, redundancy is not
5. One output line per merge group: MERGE [id1,id2,...] → merged content
6. If there is nothing to merge at all, output only NONE