// Filter and column utilities for dynamic page data binding

// Parse "status=open,priority=high" into key-value pairs
function parseFilter(filter: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const pair of filter.split(",")) {
    const eqIdx = pair.indexOf("=");
    if (eqIdx > 0) {
      const key = pair.slice(0, eqIdx).trim();
      const val = pair.slice(eqIdx + 1).trim();
      if (key) result[key] = val;
    }
  }
  return result;
}

// Filter rows matching ALL conditions in filter string
export function applyFilter<T extends Record<string, unknown>>(rows: T[], filter?: string): T[] {
  if (!filter || !filter.trim()) return rows;
  const conditions = parseFilter(filter);
  const entries = Object.entries(conditions);
  if (entries.length === 0) return rows;

  return rows.filter(row => {
    return entries.every(([key, val]) => {
      const rowVal = row[key];
      if (rowVal === null || rowVal === undefined) return false;
      return String(rowVal).toLowerCase() === val.toLowerCase();
    });
  });
}

// Pick only named columns from each row
export function pickColumns<T extends Record<string, unknown>>(
  rows: T[],
  columns?: string
): Partial<T>[] {
  if (!columns || !columns.trim()) return rows;
  const cols = columns.split(",").map(c => c.trim()).filter(Boolean);
  if (cols.length === 0) return rows;

  return rows.map(row => {
    const result: Partial<T> = {};
    for (const col of cols) {
      if (col in row) {
        (result as Record<string, unknown>)[col] = row[col];
      }
    }
    return result;
  });
}
