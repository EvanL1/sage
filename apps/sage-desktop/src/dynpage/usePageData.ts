import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { applyFilter } from "./filterUtils";

export type DataSource = "tasks" | "memories" | "feed";

interface TaskRaw {
  id: number; content: string; status: string; priority: string;
  due_date: string | null; source: string; created_at: string;
}

interface MemoryRaw {
  id: number; category: string; content: string; source: string;
  confidence: number; created_at: string; depth?: string;
}

interface FeedItemRaw {
  id: number; title: string; url: string; score: number;
  insight: string; summary: string; created_at: string;
}

type DataRow = Record<string, unknown>;

async function fetchSource(source: string): Promise<DataRow[]> {
  if (source === "tasks") {
    const raw = await invoke<TaskRaw[]>("list_tasks", { status: null, limit: 100 });
    return raw.map(t => ({ ...t } as DataRow));
  }
  if (source === "memories") {
    const raw = await invoke<MemoryRaw[]>("get_all_memories", { limit: 200 });
    return raw.map(m => ({ ...m } as DataRow));
  }
  if (source === "feed") {
    const raw = await invoke<FeedItemRaw[]>("get_feed_items", { limit: 50 });
    return raw.map(f => ({ ...f } as DataRow));
  }
  return [];
}

export function usePageData(source: string, filterStr?: string) {
  const [data, setData] = useState<DataRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!source) {
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);

    fetchSource(source)
      .then(rows => {
        setData(applyFilter(rows, filterStr));
        setLoading(false);
      })
      .catch(err => {
        setError(String(err));
        setLoading(false);
      });
  }, [source, filterStr]);

  return { data, loading, error };
}
