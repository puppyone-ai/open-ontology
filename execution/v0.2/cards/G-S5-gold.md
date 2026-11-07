## Card G-S5 — lane G: gold-question dialogue (T6) harness and first runs

TODO: S5-G-01 (plan exit: 20 gold questions answered correctly by a real-model Agent on P0 versions; failures/NULL/
truncation recorded honestly). Works ahead of the S4 exit. Rebase on v0.2/product-loops first. The data path now works
end to end except the reviewed-fence publisher (a native lane is fixing it): `tools/oo-load/test-gateway-cycles.py`
starts a real mTLS Gateway over a published oo-load generation (fixtures open fences directly).
1. A T6 harness in `tools/oo-test` (extend, don't fork): load lane H's neutral fixture with its 20 gold questions
   (`testing/engines/gold/*.sql` give the gold answers), build + review (accept-all review file) + publish, start the
   Gateway, issue a token, and give a real-model Agent only the share prompt + native CLI (`oo query` / `oo whoami`)
   to answer each question; compare against gold exactly; record NULL/truncated/refused answers separately.
2. Model: owner-private 0600 files (authorized, read via secret_ref only, never print/copy): base URL
   `~/.config/open-ontology/secrets/model-endpoint-9r`, key `~/.config/open-ontology/secrets/model-key-9r-sol`,
   model `gpt-6.1-sol`. Print the preflight; bounded calls.
3. Run on PostgreSQL 17.11 now (fence opening via the test fixture is acceptable for this first run — say so); switch
   to the real publisher entry once it is on the integration branch; then MySQL 8.4.7 and Oracle 23.5.
