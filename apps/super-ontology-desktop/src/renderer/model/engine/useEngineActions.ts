import { useCallback } from "react";
import { copyText } from "../format";
import { useSuperDriveActions, useSuperDriveSelector } from "../SuperDriveProvider";
import type { EngineActionId } from "../superDriveStateModel";

export const DOCKER_DOWNLOAD_URL = "https://www.docker.com/products/docker-desktop/";
/** Shown and copied as typed; it is a command, not translatable copy. */
export const OO_INSTALL_COMMAND = "npm install -g @reduxxx888/ontology-cli";

/**
 * Maps the action ids of the engine state matrix to store calls. `busy` is
 * true while any engine action is in flight. `run` resolves to "copied" when
 * the install command was put on the clipboard, so the caller can say so.
 */
export function useEngineActions() {
  const actions = useSuperDriveActions();
  const status = useSuperDriveSelector((state) => state.engine?.status ?? "idle");
  const busy = useSuperDriveSelector((state) => state.engineAction.busy);

  const run = useCallback(async (id: EngineActionId): Promise<"copied" | void> => {
    switch (id) {
      case "checkEnvironment":
      case "retryAfterDockerInstall":
      case "retryAfterDockerStart":
        await actions.runPreflight();
        return;
      case "retry":
        // A crashed engine is started again; every other "retry" re-checks the environment.
        await (status === "crashed" ? actions.startEngine() : actions.runPreflight());
        return;
      case "useDocker": await actions.setRuntime("docker"); return;
      case "start": await actions.startEngine(); return;
      case "stop": await actions.stopEngine(); return;
      case "restart": await actions.restartEngine(); return;
      case "chooseOo": await actions.chooseOoExecutable(); return;
      case "openDockerDownload": await actions.openExternal(DOCKER_DOWNLOAD_URL); return;
      case "viewLogs": await actions.revealEngineFolder("logs"); return;
      case "openStateFolder": await actions.revealEngineFolder("state"); return;
      case "installOo":
        await copyText(OO_INSTALL_COMMAND);
        return "copied";
      default: return;
    }
  }, [actions, status]);

  return { run, busy };
}
