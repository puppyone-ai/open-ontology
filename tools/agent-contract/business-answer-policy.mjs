import { readFile } from 'node:fs/promises';

export async function businessSystemPrompt(instructions) {
  const policy = await readFile(new URL('./business-answer-policy.md', import.meta.url), 'utf8');
  return composeBusinessSystemPrompt(instructions, policy);
}

export function composeBusinessSystemPrompt(instructions, policy, selectedSkill) {
  const generic = `${instructions}\n\n${policy}`;
  if (!selectedSkill) return generic;
  return `${generic}\n\nThe explicitly selected Skill below governs business behavior over generic guidance. `
    + `Keep the structured JSON scoring contract (including real query_ids), tool authorization and privacy restrictions. `
    + `The "oo" tool executes the operator-verified CLI mapping; do not look for a shell or infer installation failure from PATH.\n\n`
    + selectedSkill;
}
