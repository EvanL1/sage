import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { REGISTRY } from "./registry";
import type { PageNode } from "./parser";

export function renderNodes(nodes: PageNode[]): React.ReactNode {
  return nodes.map((node, i) => {
    if (node.kind === "text") {
      if (!node.content.trim()) return null;
      return (
        <ReactMarkdown key={i} remarkPlugins={[remarkGfm]}>
          {node.content}
        </ReactMarkdown>
      );
    }

    const Comp = REGISTRY[node.name];
    if (!Comp) {
      return (
        <div key={i} style={{
          padding: "6px 10px", borderRadius: 6,
          border: "1px solid var(--error, #ef4444)",
          color: "var(--error, #ef4444)", fontSize: 12, marginBottom: 8,
        }}>
          Unknown component: &lt;{node.name}&gt;
        </div>
      );
    }

    const children = node.children.length > 0
      ? renderNodes(node.children)
      : undefined;

    const props: Record<string, unknown> = { ...node.props };
    if (children !== undefined) props.children = children;

    return <Comp key={i} {...props} />;
  });
}
