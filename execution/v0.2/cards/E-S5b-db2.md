## Card E-S5b — lane E: Db2 implementation (clarified run condition)

TODO: S5-E-03. Rebase on v0.2/product-loops first (your preflight 08e661722 is integrated). Correction from the
coordinator: "run alone" on card E-S5 meant *no other container of yours* at the same time — not an idle host. Other
lanes' and unrelated long-running containers always exist on this host; leave them alone and do not wait for them.
Required instead: `df -h /` ≥ 25 GB free and Docker reporting ≥ 6 GB memory available, then run Db2 by itself among
your containers. Update `testing/engines/db2/preflight.py` accordingly.
DM8 and KingbaseES stay blocked on vendor gates (license acceptance; CAPTCHA/phone registration) — human action (D5);
do not proceed with them.
Deliver Db2 per card E-S5 item 1: connector snapshot + C5 (feature-gated module), oo-load module with one-line
dispatch, oo-test matrix entry, T0, T1 (Agent DB comparison with concurrent writer) and T8 on Db2 11.5.9.0.
