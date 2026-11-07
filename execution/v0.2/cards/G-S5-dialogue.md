## Card G-S5 — lane G: T5/T6 gold dialogue on every P0 version

TODO: S5-G-01. Lane label: `g`. Spec: AT-ENG-03, AT-ONB-01 (dialogue part).
For each P0 engine/version fixture: full Self-build → owner self-review (scripted, mode=self) → publish →
Gateway → enroll + `oo login` from a clean profile following `oo share-prompt` text → a real model agent
(gpt-6.1-sol through model-egress, tool use = the `oo` CLI only) answers the 20 gold questions. Score
against gold per question: correct / wrong / honest-unknown; NULL/failed/truncated must be reported as
such, never as 0. Report per version with transcripts in evidence; bounded model usage (one pass per
version, retries only for infrastructure failures, recorded).
