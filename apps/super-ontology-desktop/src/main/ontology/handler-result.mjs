const MARK = Symbol("super-ontology.handler-result");

/** A handler returns this when the engine answered from the idempotency store (`OO-Idempotent-Replay`). */
export function handlerResult(value, { replayed = false } = {}) {
  return Object.freeze({ [MARK]: true, value, replayed });
}

export function unwrapHandlerResult(result) {
  return result && typeof result === "object" && result[MARK] === true
    ? { value: result.value, replayed: result.replayed }
    : { value: result, replayed: false };
}
