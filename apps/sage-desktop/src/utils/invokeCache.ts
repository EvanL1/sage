import { invoke } from "@tauri-apps/api/core";

const cache = new Map<string, { promise: Promise<unknown>; ts: number }>();
const DEFAULT_TTL = 30_000; // 30s — matches daemon heartbeat

/**
 * Deduplicated invoke: same cmd+args within TTL share one Promise.
 * Prevents N widgets from triggering N identical Tauri IPC calls.
 */
export function invokeDeduped<T>(cmd: string, args?: Record<string, unknown>, ttl = DEFAULT_TTL): Promise<T> {
  const key = cmd + (args ? JSON.stringify(args) : "");
  const hit = cache.get(key);
  if (hit && Date.now() - hit.ts < ttl) return hit.promise as Promise<T>;
  const promise = invoke<T>(cmd, args);
  cache.set(key, { promise, ts: Date.now() });
  // Evict on error so retries work
  promise.catch(() => cache.delete(key));
  return promise;
}

/** Force-invalidate a specific command's cache (call after mutations) */
export function invalidateCache(cmd: string, args?: Record<string, unknown>) {
  const key = cmd + (args ? JSON.stringify(args) : "");
  cache.delete(key);
}

/** Invalidate all cached entries matching a prefix (e.g., "get_memories") */
export function invalidateCachePrefix(prefix: string) {
  for (const key of cache.keys()) {
    if (key.startsWith(prefix)) cache.delete(key);
  }
}

/** Clear entire cache (call on page navigation or manual refresh) */
export function clearCache() {
  cache.clear();
}
