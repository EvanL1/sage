# Memory Provenance Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Sage's memory evolution transparent and traceable — every memory knows where it came from, every evolution is auditable, and destructive changes become soft.

**Architecture:** Add `derived_from` + `evolution_note` columns to memories table, change DEDUP from hard delete to soft archive, and generate a human-readable evolution summary. No new tables.

**Tech Stack:** SQLite migration, Rust (sage-store, sage-core, sage-types)

---

### Task 1: Migration — Add provenance columns

**Files:**
- Modify: `crates/sage-store/src/migrations.rs` (append v38)

**Step 1: Write migration v38**

Add at the end of `migrate()`, before the compensation block:

```rust
if version < 38 {
    conn.execute_batch(
        "ALTER TABLE memories ADD COLUMN derived_from TEXT;
         ALTER TABLE memories ADD COLUMN evolution_note TEXT;
         PRAGMA user_version = 38;",
    )
    .context("数据库迁移 v38（memory provenance）失败")?;
}
```

**Step 2: Verify**

Run: `cargo check -p sage-store`
Expected: clean build

**Step 3: Commit**

```bash
git add crates/sage-store/src/migrations.rs
git commit -m "feat: migration v38 — add derived_from and evolution_note columns"
```

---

### Task 2: Update Memory struct in sage-types

**Files:**
- Modify: `crates/sage-types/src/lib.rs` (Memory struct, around line 278)

**Step 1: Add fields to Memory struct**

After `pub embedding: Option<Vec<u8>>` (line 306), add:

```rust
/// 溯源：这条记忆由哪些记忆演化而来（JSON 数组 "[12, 47, 83]"）
#[serde(default, skip_serializing_if = "Option::is_none")]
pub derived_from: Option<String>,
/// 演化备注：为什么产生这条变更
#[serde(default, skip_serializing_if = "Option::is_none")]
pub evolution_note: Option<String>,
```

**Step 2: Verify**

Run: `cargo check -p sage-types`
Expected: clean build (all downstream crates will need query updates, checked in next tasks)

---

### Task 3: Update Store queries to read/write provenance fields

**Files:**
- Modify: `crates/sage-store/src/memories.rs`

This task updates all Memory-reading queries to include the two new columns, and adds a new method for evolution writes.

**Step 1: Update `row_to_memory` helper (or all query mappings)**

Find every place where a `Memory` struct is constructed from a row. Add:
```rust
derived_from: row.get("derived_from").ok(),
evolution_note: row.get("evolution_note").ok(),
```

Key functions to update:
- `load_active_memories()` — SELECT must include `derived_from, evolution_note`
- `search_memories()` — same
- `get_memory_by_id()` — same
- `get_memories_since()` — same
- `load_memories_by_depth()` — same
- Any other function returning `Vec<Memory>` or `Memory`

**Step 2: Add `archive_memory_with_provenance` method**

```rust
/// 软删除记忆（归档而非硬删除），并在新记忆上记录溯源
pub fn archive_memory(&self, id: i64, note: &str) -> Result<()> {
    let conn = self.conn.lock()
        .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
    let now = chrono::Local::now().to_rfc3339();
    conn.execute(
        "UPDATE memories SET status = 'archived', evolution_note = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![note, now, id],
    ).context("归档 memory 失败")?;
    Ok(())
}
```

**Step 3: Add `save_memory_with_provenance` method**

```rust
/// 保存新记忆并记录溯源（由 Evolution COMPILE 使用）
pub fn save_memory_with_provenance(
    &self,
    category: &str,
    content: &str,
    source: &str,
    confidence: f64,
    derived_from_ids: &[i64],
    note: &str,
) -> Result<i64> {
    let id = self.save_memory(category, content, source, confidence)?;
    if id > 0 {
        let derived_json = serde_json::to_string(derived_from_ids).unwrap_or_default();
        let conn = self.conn.lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE memories SET derived_from = ?1, evolution_note = ?2 WHERE id = ?3",
            rusqlite::params![derived_json, note, id],
        ).context("更新 provenance 失败")?;
    }
    Ok(id)
}
```

**Step 4: Verify**

Run: `cargo check -p sage-store`
Expected: clean build

---

### Task 4: Write tests for provenance store methods

**Files:**
- Modify: `crates/sage-store/src/memories.rs` (test module at bottom)

**Step 1: Write failing tests**

```rust
#[test]
fn test_archive_memory() {
    let store = Store::open_in_memory().unwrap();
    let id = store.save_memory("behavior", "test content", "chat", 0.7).unwrap();
    store.archive_memory(id, "merged with #99").unwrap();
    // 归档后不应出现在 active memories
    let active = store.load_active_memories().unwrap();
    assert!(active.iter().all(|m| m.id != id));
}

#[test]
fn test_save_memory_with_provenance() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.save_memory("behavior", "loves coffee", "chat", 0.7).unwrap();
    let id2 = store.save_memory("behavior", "drinks espresso daily", "chat", 0.6).unwrap();
    let new_id = store.save_memory_with_provenance(
        "behavior", "caffeine dependent", "evolution", 0.8,
        &[id1, id2], "合并自#id1和#id2，咖啡相关行为",
    ).unwrap();
    assert!(new_id > 0);
    // 验证 provenance 字段
    let mem = store.get_memory_by_id(new_id).unwrap().unwrap();
    assert!(mem.derived_from.as_ref().unwrap().contains(&id1.to_string()));
    assert_eq!(mem.evolution_note.as_deref(), Some("合并自#id1和#id2，咖啡相关行为"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sage-store -- test_archive_memory test_save_memory_with_provenance`
Expected: FAIL (methods don't exist yet or queries don't return new fields)

**Step 3: Implement until tests pass**

Run: `cargo test -p sage-store -- test_archive_memory test_save_memory_with_provenance`
Expected: PASS

**Step 4: Run full store test suite**

Run: `cargo test -p sage-store`
Expected: all 188+ tests pass

**Step 5: Commit**

```bash
git add crates/sage-store/src/memories.rs
git commit -m "feat: add archive_memory and save_memory_with_provenance store methods"
```

---

### Task 5: Change Evolution DEDUP from hard delete to soft archive

**Files:**
- Modify: `crates/sage-core/src/memory_evolution.rs` (lines 107-116)

**Step 1: Change DEDUP handler**

Replace the current DEDUP block:
```rust
// Before:
for id in &ids { let _ = store.delete_memory(*id); }
```

With:
```rust
// After:
for id in &ids {
    let _ = store.archive_memory(*id, "dedup: 与其他记忆重复");
}
```

**Step 2: Change COMPILE handler to use provenance**

Replace the current save_memory + mark_memory_compiled block (lines 131-142) with:
```rust
if let Ok(new_id) = store.save_memory_with_provenance(
    category, content, "evolution", conf,
    &ids,
    &format!("compile: {}条记忆→{depth}:{category}", ids.len()),
) {
    if new_id > 0 {
        for &id in &ids { let _ = store.archive_memory(id, &format!("compiled into #{new_id}")); }
        // ... rest of depth/counter logic
    }
}
```

**Step 3: Change BELIEF handler similarly**

Same pattern: `save_memory_with_provenance` + `archive_memory` on sources.

**Step 4: Verify**

Run: `cargo check -p sage-core`
Expected: clean build

Run: `cargo test -p sage-core -- evolution`
Expected: tests pass (DEDUP test asserts `active.len()` decreases, which still works with soft delete since archived != active)

**Step 5: Commit**

```bash
git add crates/sage-core/src/memory_evolution.rs
git commit -m "feat: evolution DEDUP/COMPILE now soft-archive with provenance tracking"
```

---

### Task 6: Add evolution summary to EvolutionResult

**Files:**
- Modify: `crates/sage-core/src/memory_evolution.rs`

**Step 1: Add summary field to EvolutionResult**

```rust
pub struct EvolutionResult {
    // ... existing fields ...
    /// 面向用户的演化摘要
    pub summary: String,
}
```

**Step 2: Generate summary at end of `evolve()`**

Before returning `Ok(EvolutionResult { ... })`, build a human-readable summary:

```rust
let mut summary_parts = Vec::new();
if merged > 0 { summary_parts.push(format!("去重 {merged} 条")); }
if compiled_semantic > 0 { summary_parts.push(format!("归纳 {compiled_semantic} 条模式")); }
if compiled_axiom > 0 { summary_parts.push(format!("凝结 {compiled_axiom} 条核心信念")); }
if condensed > 0 { summary_parts.push(format!("压缩 {condensed} 条")); }
if reclassified > 0 { summary_parts.push(format!("重分类 {reclassified} 条")); }
if linked > 0 { summary_parts.push(format!("连接 {linked} 对关系")); }
let summary = if summary_parts.is_empty() {
    "无变更".to_string()
} else {
    format!("今日演化：{}", summary_parts.join("，"))
};
```

**Step 3: Update EvolutionResult construction to include summary**

**Step 4: Update the Tauri command to return the summary**

In `apps/sage-desktop/src-tauri/src/commands/reports.rs`, the `trigger_memory_evolution` handler already formats a toast message. Replace the current formatting with `r.summary`.

**Step 5: Verify**

Run: `cargo check -p sage-core && cargo check -p sage-desktop`
Expected: clean build

**Step 6: Commit**

```bash
git add crates/sage-core/src/memory_evolution.rs apps/sage-desktop/src-tauri/src/commands/reports.rs
git commit -m "feat: evolution returns human-readable summary for user notification"
```

---

### Task 7: Final verification

**Step 1: Full build**

Run: `cargo check`

**Step 2: Full test suite**

Run: `cargo test -p sage-core && cargo test -p sage-store`

**Step 3: Frontend check**

Run: `cd apps/sage-desktop && npx tsc --noEmit`

**Step 4: Commit if any fixups needed**
