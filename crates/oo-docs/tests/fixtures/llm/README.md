# Recorded model replies

`replies.json` holds the model replies the LLM tests replay; no test calls a
model. A reply is a template: `{{BLOCK:<text>}}` stands for the id of the block
whose text starts with `<text>` (ids come from the anchor and text, so the
template cannot carry them). `pinned_keys` are the cache keys of the two
batches of `fixture.md`: a change to the prompt, the batching or the key
serialisation changes them and fails the test, the same way a recorded prompt
drift fails `gate-ab-replay`.

Re-pinning after a prompt edit (the cache key hashes the whole system text plus
`prompt_version`, so any edit already invalidates every cached reply, and
`prompt.rs` pins the pair of version and system text hash so a reworded rule
cannot keep an old version name):

1. bump `prompt_version` (`extract/N`) and pin the new version and the new
   `blake3(SYSTEM)` in `an_edit_of_the_system_text_comes_with_a_new_prompt_version`;
2. empty `pinned_keys` in `replies.json` and run
   `OO_DOCS_PRINT_KEYS=1 OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test llm_live -- --ignored --test-threads=1 recorded_replies`
   to print the keys of the two batches, then put them back.

The replies are templates replayed by user message, not captured model output,
so re-pinning changes keys only; no reply text is edited to fit a key.
