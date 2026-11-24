import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

/**
 * Fake `docker` and `oo` executables for main-process tests (no real Docker,
 * no network). Behaviour is steered by the environment handed to the probes,
 * which is exactly how the real tools are found: through the user's PATH.
 */
const FAKE_DOCKER = `#!/bin/sh
case "$1" in
  version) echo '{"Client":{"Version":"27.3.1"}}' ;;
  info)
    # Running when the test says so by environment, or by a marker file next to this script (T1 flips it live).
    if [ "$FAKE_DOCKER_RUNNING" = "1" ] || [ -f "$(dirname "$0")/docker-running" ]; then echo '{"ServerVersion":"27.3.1"}'; else echo "Cannot connect to the Docker daemon" >&2; exit 1; fi ;;
  *) exit 2 ;;
esac
`;
const FAKE_DOCKER_HANGING = "#!/bin/sh\nsleep 30\n";
const FAKE_OO = `#!/bin/sh
if [ "$1" = "--version" ]; then echo '{"version":"0.3.0"}'; exit 0; fi
exit 0
`;
const FAKE_OO_SCRIPT = new URL("./fake-oo.mjs", import.meta.url).pathname;
const FAKE_OO_DAEMON = `#!/bin/sh\nexec "${process.execPath}" "${FAKE_OO_SCRIPT}" "$@"\n`;
const BROKEN_OO = "#!/bin/sh\necho 'not a version' >&2\nexit 1\n";
const HANGING_OO = "#!/bin/sh\nsleep 30\n";

async function writeExecutable(directory, name, source) {
  const target = path.join(directory, name);
  await writeFile(target, source);
  await chmod(target, 0o755);
  return target;
}

export async function createFakeToolbox() {
  const root = await mkdtemp(path.join(os.tmpdir(), "so-fake-tools-"));
  const bin = path.join(root, "bin");
  await mkdir(bin);
  return {
    root,
    bin,
    /** Returns an environment whose PATH is only the fake bin directory. */
    environment: (extra = {}) => ({ PATH: bin, ...extra }),
    install: (name, kind) => writeExecutable(bin, name, {
      docker: FAKE_DOCKER, "fake-oo": FAKE_OO_DAEMON, "docker-hanging": FAKE_DOCKER_HANGING, oo: FAKE_OO, "oo-broken": BROKEN_OO, "oo-hanging": HANGING_OO,
    }[kind]),
    /** An `oo` placed outside PATH, for the "choose location" flow. */
    installElsewhere: async (name, kind) => {
      const directory = path.join(root, "elsewhere");
      await mkdir(directory, { recursive: true });
      return writeExecutable(directory, name, { oo: FAKE_OO, "oo-broken": BROKEN_OO }[kind]);
    },
    dispose: () => rm(root, { recursive: true, force: true }),
  };
}
