# Failed attempts

No test, clippy, smoke, or postgres start failed. Every `*.exit` listed in `RAW-EXITS.txt` at the time of the proof is 0.

The password redactor in `linux/run-proof.sh` replaces any run of 48 hex characters. SHA-256 is 64 hex characters, so `proof/source-sha256.txt` lost the first 48 characters of each digest. The `sha256sum` command itself exited 0 (`proof/source-sha256.txt.exit`). Before cleanup, the same builder ran `sha256sum` again on the mounted `/src` tree with no redactor. That output is `proof/source-sha256-full.txt`. The migration digests there match the contract values.

A manual `sha256sum` from the container's default directory `/` failed with "No such file or directory" and was not part of the proof. The follow-up used `-w /src`.
