import path from "node:path";
import { SuperOntologyError } from "../../../src/shared/super-ontology/errors.mjs";
import { driveFileUrl } from "./drive-file-protocol.mjs";
import { ensure, isDrive, isFileSummary, isPage } from "./response-guards.mjs";

/** The two detail documents the Drive file pane reads, and the capability URL of a file's original. */
export function createFileHandlers({ client, tokens }) {
  const call = (name, options) => client.request(name, options);

  return {
    /** `GET files/{fid}` with the requested includes; the document is passed on as the engine sent it. */
    "file-detail": async ({ fileId, include }) => {
      const { value } = await call("fileDetail", { params: { fid: fileId }, query: { include } });
      return ensure(value, isFileSummary);
    },

    /** One page of the parsed blocks (`409 DOCS_PARSE_NOT_READY` reaches the renderer as that code). */
    "file-blocks": async ({ fileId, page, limit, cursor }) => {
      const { value } = await call("fileBlocks", { params: { fid: fileId }, query: { page, limit, cursor } });
      return ensure(value, isPage);
    },

    /** Only fs Drives have an original on this computer; s3 Drives answer SO_PREVIEW_UNAVAILABLE. */
    "file-preview-url": async ({ fileId }) => {
      const { value: file } = await call("fileDetail", { params: { fid: fileId }, query: {} });
      ensure(file, isFileSummary);
      const { value: drive } = await call("driveGet", { params: { id: file.drive_id } });
      ensure(drive, isDrive);
      const root = drive.config?.root;
      if (drive.backend !== "fs" || typeof root !== "string" || !path.isAbsolute(root) || typeof file.path !== "string") throw new SuperOntologyError("SO_PREVIEW_UNAVAILABLE");
      const { token, expiresAt } = tokens.issue({ root, relativePath: file.path });
      return { url: driveFileUrl(token, path.posix.basename(file.path)), expiresAt: new Date(expiresAt).toISOString() };
    },
  };
}
