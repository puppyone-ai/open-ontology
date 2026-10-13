# DL01-W1-14 MongoDB target preparation

Synthetic standalone MongoDB 8.0.32 used to pin the image, separate a DBA from a reader and an audit identity, and exercise catalog, batch, cancel, and change-stream refusal. This directory is preparation. It is not source-support acceptance and it does not promote the support matrix.

The product driver is absent at base `577160c22de63adb9b38e0147f3bc2da893c4d9d`. `mongosh` 2.12.0 is the probe client only. Canonical extended JSON (`relaxed: false`) is the preservation format. Relaxed JSON is retained as a negative sample because int64 above 2^53 becomes an IEEE number.

## Run

The container `oo-dl01w1-14-mongodb` must already be running with `/run/secrets/{dba,reader,audit}` mounted read-only. Passwords are not command arguments.

```sh
python3 -m unittest testing/dl01-targets/mongodb/tests/test_oracle_rules.py
sh testing/dl01-targets/mongodb/scripts/run-journey.sh evidence/df309940-5694-474f-a7ef-e9f764abf09c/DL01/mongodb
```

The unit test does not open MongoDB. The journey exit is 0 only when DBA setup, the reader, the audit reader, the oracle, and the secret scan are all 0.

`scripts/cleanup.sh` removes only `oo-dl01w1-14-mongodb`, its anonymous configdb volume, `oo-dl01w1-14-data`, and `oo-dl01w1-14-net`. It does not delete the image.
