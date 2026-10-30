// Shared model input space for the real CLI, Demo, and cold acceptance runner.
export function searchSchema(Type) {
  return {
    path_prefix: Type.Optional(Type.String({ maxLength: 256, description: 'Used only by knowledge_search; empty means no path filter.' })),
    offset: Type.Optional(Type.Integer({ minimum: 0, maximum: 1048576, description: 'Used only by knowledge_search.' })),
    limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 100, description: 'Used only by knowledge_search.' })),
  };
}

export function searchFlags(args) {
  if (args.operation !== 'knowledge_search') {
    // Compatible models often fill optional defaults on every call. They have
    // no meaning for other operations and must never become CLI flags there.
    return [];
  }
  const flags = [];
  if (args.path_prefix !== undefined && args.path_prefix !== null && args.path_prefix !== '') {
    const prefix = args.path_prefix;
    if (typeof prefix !== 'string' || !prefix || prefix.length > 256 ||
        !prefix.replace(/\/$/, '').split('/').every(part =>
          part && part !== '.' && part !== '..' && /^[A-Za-z0-9_.-]+$/.test(part))) {
      throw new Error('INVALID_SEARCH_PREFIX');
    }
    flags.push('--path-prefix', prefix);
  }
  for (const [name, low, high] of [['offset', 0, 1048576], ['limit', 1, 100]]) {
    if (args[name] !== undefined && args[name] !== null) {
      if (!Number.isSafeInteger(args[name]) || args[name] < low || args[name] > high) {
        throw new Error('INVALID_SEARCH_PAGE');
      }
      flags.push(`--${name}`, String(args[name]));
    }
  }
  return flags;
}
