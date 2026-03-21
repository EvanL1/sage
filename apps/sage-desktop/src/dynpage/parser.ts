// Lightweight parser: Enhanced Markdown + PascalCase component tags → PageNode tree

export type PageNode =
  | { kind: "text"; content: string }
  | { kind: "component"; name: string; props: Record<string, string>; children: PageNode[] };

function parseProps(raw: string): Record<string, string> {
  const props: Record<string, string> = {};
  const re = /(\w+)="([^"]*)"/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(raw)) !== null) {
    props[m[1]] = m[2];
  }
  return props;
}

// Returns [componentName, propsStr, selfClosing, charsConsumed] or null
function tryParseOpenTag(src: string, pos: number): [string, string, boolean, number] | null {
  // pos is right after '<'
  const rest = src.slice(pos);
  const nameMatch = rest.match(/^([A-Z][A-Za-z0-9]*)/);
  if (!nameMatch) return null;

  const name = nameMatch[1];
  let i = name.length;
  // scan to '>' or '/>'
  while (i < rest.length && rest[i] !== ">" && !(rest[i] === "/" && rest[i + 1] === ">")) {
    i++;
  }
  if (i >= rest.length) return null;

  const propsStr = rest.slice(name.length, i).trim();
  const selfClosing = rest[i] === "/";
  const advance = i + (selfClosing ? 2 : 1);
  return [name, propsStr, selfClosing, pos + advance];
}

/** Serialize a node tree back to Enhanced Markdown */
export function serializeNodes(nodes: PageNode[]): string {
  return nodes.map(n => {
    if (n.kind === "text") return n.content;
    const propsStr = Object.entries(n.props).map(([k, v]) => `${k}="${v}"`).join(" ");
    const tag = n.name;
    if (n.children.length === 0) {
      return `<${tag}${propsStr ? " " + propsStr : ""} />`;
    }
    const inner = serializeNodes(n.children);
    return `<${tag}${propsStr ? " " + propsStr : ""}>${inner}</${tag}>`;
  }).join("\n");
}

export function parsePage(src: string): PageNode[] {
  const nodes: PageNode[] = [];
  let i = 0;
  let textStart = 0;
  let inCodeBlock = false;

  const flushText = (end: number) => {
    if (end > textStart) {
      const content = src.slice(textStart, end);
      if (content) nodes.push({ kind: "text", content });
    }
    textStart = end;
  };

  while (i < src.length) {
    // Track code blocks (``` at line start)
    if (src[i] === "`" && src[i + 1] === "`" && src[i + 2] === "`") {
      inCodeBlock = !inCodeBlock;
      i += 3;
      continue;
    }

    if (src[i] === "<" && !inCodeBlock) {
      const result = tryParseOpenTag(src, i + 1);
      if (result) {
        const [name, propsStr, selfClosing, afterOpen] = result;
        flushText(i);

        const props = parseProps(propsStr);
        if (selfClosing) {
          nodes.push({ kind: "component", name, props, children: [] });
          i = afterOpen;
          textStart = i;
        } else {
          // Find content between open and close tags
          const closeTagStr = `</${name}>`;
          const closeIdx = src.indexOf(closeTagStr, afterOpen);
          if (closeIdx === -1) {
            // Malformed — treat as text
            i++;
          } else {
            const innerSrc = src.slice(afterOpen, closeIdx);
            const children = parsePage(innerSrc);
            nodes.push({ kind: "component", name, props, children });
            i = closeIdx + closeTagStr.length;
            textStart = i;
          }
        }
        continue;
      }
    }

    i++;
  }

  flushText(src.length);
  return nodes;
}
