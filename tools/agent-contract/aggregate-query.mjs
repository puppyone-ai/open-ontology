// Privacy guard for evaluation, not SQL authorization. Gateway validation still applies.
function splitExpressions(text) {
  const out = [];
  let depth = 0, quoted = false, start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      if (quoted && text[i + 1] === '"') i++;
      else quoted = !quoted;
    } else if (!quoted) {
      if (c === '(') depth++;
      if (c === ')' && --depth < 0) return null;
      if (c === ',' && depth === 0) { out.push(text.slice(start, i).trim()); start = i + 1; }
    }
  }
  if (quoted || depth !== 0) return null;
  out.push(text.slice(start).trim());
  return out;
}

function aggregateExpression(text) {
  const tokens = [];
  while (text.trim()) {
    text = text.trimStart();
    const match = text.match(/^(?:"(?:[^"]|"")*"|[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?|[().,+*/-])/);
    if (!match) return false;
    tokens.push(match[0]); text = text.slice(match[0].length);
  }
  let i = 0, depth = 0;
  const name = value => typeof value === 'string' && /^(?:[A-Za-z_][A-Za-z0-9_]*|"(?:[^"]|"")*")$/.test(value);
  const eat = token => tokens[i] === token ? (i++, true) : false;
  function reference() {
    if (!name(tokens[i]) || /^(SELECT|FROM|DISTINCT|AS|NULL)$/i.test(tokens[i])) return false;
    i++;
    let parts = 1;
    while (eat('.')) { if (++parts > 3 || !name(tokens[i++])) return false; }
    return true;
  }
  function atom() {
    if (++depth > 32) return false;
    let valid;
    if (eat('(')) valid = expression() && eat(')');
    else if (tokens[i] === '+' || tokens[i] === '-') { i++; valid = atom(); }
    else if (/^\d+(?:\.\d+)?$/.test(tokens[i] || '')) { i++; valid = true; }
    else if (/^COALESCE$/i.test(tokens[i] || '') && tokens[i + 1] === '(') {
      i += 2;
      valid = expression() && eat(',') && expression();
      while (valid && eat(',')) valid = expression();
      valid = valid && eat(')');
    } else valid = reference();
    depth--;
    return valid;
  }
  function expression() {
    if (!atom()) return false;
    while (['+', '-', '*', '/'].includes(tokens[i])) { i++; if (!atom()) return false; }
    return true;
  }
  const aggregate = tokens[i++]?.toUpperCase();
  if (!['COUNT', 'SUM', 'AVG', 'MIN', 'MAX'].includes(aggregate) || !eat('(')) return false;
  if (aggregate === 'COUNT' && eat('*')) return eat(')') && i === tokens.length;
  if (aggregate === 'COUNT' && tokens[i]?.toUpperCase() === 'DISTINCT') {
    i++;
    return reference() && eat(')') && i === tokens.length;
  }
  return expression() && eat(')') && i === tokens.length;
}

export function aggregateOnly(sql) {
  if (typeof sql !== 'string' || sql.length > 65536 ||
      /;|--|\/\*|\b(WITH|UNION|INTERSECT|EXCEPT|OVER|SELECT\b[\s\S]*SELECT)\b/i.test(sql)) return false;
  const match = sql.match(/^\s*SELECT\s+([\s\S]+?)\s+FROM\s/i);
  if (!match || !/\b(COUNT|SUM|AVG|MIN|MAX)\s*\(/i.test(match[1])) return false;
  const projections = splitExpressions(match[1]);
  const groups = splitExpressions(sql.match(/\bGROUP\s+BY\s+([\s\S]+?)(?:\s+ORDER\s+BY|\s+LIMIT|$)/i)?.[1] || '');
  if (!projections || !groups) return false;
  return projections.every(part => {
    const expression = part.replace(/\s+AS\s+[A-Za-z_][A-Za-z0-9_]*$/i, '').trim();
    return aggregateExpression(expression) ||
      (/^(?:[\w"]+\.)?[\w"]+$/.test(expression) && groups.includes(expression));
  });
}
