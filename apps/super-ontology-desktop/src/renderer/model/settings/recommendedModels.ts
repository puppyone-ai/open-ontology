/**
 * Models the connection dialog proposes. The first one is what a new connection starts with (measured on the project's own
 * evaluation, see testing/super-ontology/evidence/L4/eval/model-choice/README.md); the others are offered while typing.
 * A name is only a suggestion: the provider decides which names exist, and any name can be typed.
 */
export const RECOMMENDED_MODEL = "cb/gpt-6.2-sol";
export const SUGGESTED_MODELS: readonly string[] = [RECOMMENDED_MODEL, "agm/gemini-3.8-flash-low", "gcli/grok-4.7-build-fast"];
