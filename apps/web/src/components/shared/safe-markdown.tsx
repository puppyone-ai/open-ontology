export type Citation = {
  id: string;
  href: string;
};

function safeHref(href: string): string | null {
  if (!href.startsWith("/") || href.startsWith("//") || href.includes("\\") || href.includes("://")) {
    return null;
  }
  return href;
}

function renderInline(text: string, citations: readonly Citation[], keyPrefix: string) {
  const parts = text.split(/(`[^`]+`|\[\^[^\]]+\])/g);
  return parts.map((part, index) => {
    const key = `${keyPrefix}-${index}`;
    if (part.startsWith("`") && part.endsWith("`") && part.length >= 2) {
      return <code key={key}>{part.slice(1, -1)}</code>;
    }
    const citation = /^\[\^([^\]]+)\]$/.exec(part);
    if (citation) {
      const id = citation[1] ?? "";
      const match = citations.find((item) => item.id === id);
      const href = match ? safeHref(match.href) : null;
      if (href) {
        return (
          <a key={key} href={href}>
            {part}
          </a>
        );
      }
      return <span key={key}>{part}</span>;
    }
    return <span key={key}>{part}</span>;
  });
}

/** Renders text only. HTML, images, and unlisted citations stay inert. */
export function SafeMarkdown({ source, citations = [] }: { source: string; citations?: readonly Citation[] }) {
  const blocks = source.split(/\n{2,}/);
  return (
    <div className="oo-stack">
      {blocks.map((block, index) => {
        if (block.startsWith("```") && block.endsWith("```")) {
          const inner = block.replace(/^```[^\n]*\n?/, "").replace(/```$/, "");
          return (
            <pre key={index} className="oo-code">
              {inner}
            </pre>
          );
        }
        return <p key={index}>{renderInline(block, citations, String(index))}</p>;
      })}
    </div>
  );
}
