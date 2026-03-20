import type { DisplayItem } from "./types";

const STORAGE_KEY = "sage_pinned_items";
const EVENT_NAME = "sage-pin-changed";

export interface PinnedItem extends DisplayItem {
  pinnedAt: string;
}

function notify() { window.dispatchEvent(new Event(EVENT_NAME)); }

export function onPinChange(cb: () => void): () => void {
  window.addEventListener(EVENT_NAME, cb);
  return () => window.removeEventListener(EVENT_NAME, cb);
}

export function loadPinned(): PinnedItem[] {
  try { const s = localStorage.getItem(STORAGE_KEY); if (s) return JSON.parse(s); } catch {}
  return [];
}

function itemKey(item: DisplayItem): string | number {
  return item.id ?? item.ref_id ?? item.content.slice(0, 80);
}

export function pinItem(item: DisplayItem): PinnedItem[] {
  const existing = loadPinned();
  const key = itemKey(item);
  if (existing.some(p => itemKey(p) === key)) return existing;
  const next = [{ ...item, pinnedAt: new Date().toISOString() }, ...existing];
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  notify();
  return next;
}

export function unpinItem(index: number): PinnedItem[] {
  const existing = loadPinned();
  existing.splice(index, 1);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(existing));
  notify();
  return [...existing];
}

export function isPinned(item: DisplayItem): boolean {
  const key = itemKey(item);
  return loadPinned().some(p => itemKey(p) === key);
}

export function togglePin(item: DisplayItem): PinnedItem[] {
  const key = itemKey(item);
  const existing = loadPinned();
  const idx = existing.findIndex(p => itemKey(p) === key);
  if (idx >= 0) return unpinItem(idx);
  return pinItem(item);
}
