# Failed attempts

No builder attempt failed. `linux/build-1/docker-run.exit` is 0. Phase exits in `linux/raw/raw-exits.txt` are all 0.

The evidence probe called `Store::finish_login` on a value returned by `Store::connect` before the container started. No compile was spent on an earlier draft of that call. Product sources were not edited.
