/**
 * Incremental `text/event-stream` parser (10 §8.5): `id`, `event`, `data`
 * (multi-line), `retry`, and `:` comment lines such as `: ping`. A message is
 * dispatched at each blank line and only when it carries data. Chunks may
 * split anywhere, including inside a CRLF pair.
 */
export function createSseParser({ onMessage, onRetry = () => {}, onComment = () => {} }) {
  let buffer = "";
  let data = [];
  let eventName = "";
  let id;

  function dispatch() {
    if (data.length === 0) return;
    onMessage({ event: eventName || "message", data: data.join("\n"), id });
    data = [];
    eventName = "";
    id = undefined;
  }

  function line(text) {
    if (text === "") { dispatch(); return; }
    if (text.startsWith(":")) { onComment(text.slice(1).trim()); return; }
    const colon = text.indexOf(":");
    const name = colon === -1 ? text : text.slice(0, colon);
    let value = colon === -1 ? "" : text.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (name === "data") data.push(value);
    else if (name === "event") eventName = value;
    else if (name === "id") { if (!value.includes("\0")) id = value; }
    else if (name === "retry" && /^\d+$/u.test(value)) onRetry(Number(value));
  }

  return {
    feed(chunk) {
      buffer += chunk;
      for (;;) {
        const match = /\r\n|\n|\r/u.exec(buffer);
        if (!match) return;
        // A lone trailing CR may be the first half of CRLF: wait for the next chunk.
        if (match[0] === "\r" && match.index + 1 === buffer.length) return;
        const text = buffer.slice(0, match.index);
        buffer = buffer.slice(match.index + match[0].length);
        line(text);
      }
    },
  };
}
