# Recorded model replies (`extract/2`)

`replies.json` holds the model replies the LLM tests replay; no test calls a
model. A reply is a template: `{{BLOCK:<text>}}` stands for the id of the block
whose text starts with `<text>` (ids come from the anchor and text, so the
template cannot carry them). `pinned_keys` are the cache keys of the two
batches of `fixture.md`: a change to the prompt, the batching or the key
serialisation changes them and fails the test, the same way a recorded prompt
drift fails `gate-ab-replay`.
