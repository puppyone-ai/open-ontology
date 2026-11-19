import { unwrapHandlerResult } from "./super-ontology/handler-result.mjs";
import { toErrorResult } from "../../shared/super-ontology/errors.mjs";
import {
  SUPER_ONTOLOGY_CHANNEL_PREFIX,
  SUPER_ONTOLOGY_CHANNELS,
  parseSuperOntologyRequest,
} from "../../shared/super-ontology/requests.mjs";

/**
 * Register only on the application's trusted-frame IPC facade, never on raw
 * ipcMain. With the capability off (`available` false or no service) nothing
 * is registered. Returns the registered channel names.
 */
export function registerSuperOntologyIpcHandlers({ ipcMain, service, available = true }) {
  if (!available || !service) return Object.freeze([]);
  const registered = [];
  for (const channel of SUPER_ONTOLOGY_CHANNELS) {
    ipcMain.handle(`${SUPER_ONTOLOGY_CHANNEL_PREFIX}${channel}`, async (event, raw) => {
      try {
        const request = parseSuperOntologyRequest(channel, raw);
        const { value, replayed } = unwrapHandlerResult(await service.handlers[channel](request, { sender: event?.sender }));
        return replayed ? { ok: true, value, replayed: true } : { ok: true, value };
      } catch (error) {
        // Never serialize native errors; a remote party may echo Authorization.
        return toErrorResult(error);
      }
    });
    registered.push(channel);
  }
  return Object.freeze(registered);
}
