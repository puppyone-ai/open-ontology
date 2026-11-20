/** The atlas data model (D88, 25 section 7.3): what the adapter produces and the scene consumes. Plain data, no engine types. */

/** One cool low-saturation family around the ink-blue accent plus warm neutrals; never the status colours. */
export type AtlasTone = "ink" | "steel" | "dusk" | "mist" | "sand" | "stone";

import type { UnlinkedReason } from "../../model/unlinked";

/**
 * `type`: an entity type (members are entities). `drive`: a document source (members are files). `db`: a database (members are tables).
 * `unlinked`: the quiet side area of what is not connected to anything (files and entities, each with its reason).
 */
export type AtlasGroupKind = "type" | "drive" | "db" | "unlinked";

export type AtlasRef =
  | { kind: "entity"; key: string }
  | { kind: "file"; fileId: string; path: string }
  | { kind: "table"; table: string };

/** What kind of source a `drive` or `db` group is: My files, a local folder, an object-store bucket, a database. */
export type AtlasSourceKind = "files" | "folder" | "bucket" | "database";
export type AtlasGroup = { id: string; kind: AtlasGroupKind; label: string; tone: AtlasTone; source?: AtlasSourceKind };

export const SOURCE_TONE: Record<AtlasSourceKind, AtlasTone> = { files: "ink", folder: "dusk", bucket: "sand", database: "mist" };

/** Where an entity comes from: a document, a database, or both (the same thing seen on both sides). Drawn as a dot, a square, a dot in a ring. */
export type AtlasOrigin = "doc" | "db" | "both";

export type AtlasMember = {
  id: string;
  group: string;
  label: string;
  ref: AtlasRef;
  /** Facts (entities), versions or rows (tables): sizes the cell, never the colour. */
  weight: number;
  /** The ontology version in which it first appeared, when the engine says; `null` otherwise. */
  born: number | null;
  /** When the engine says it; otherwise the scene derives it from the reference (tables are database, the rest documents). */
  origin?: AtlasOrigin;
  /** Why the member is in the not-connected group; absent for every other member. */
  reason?: UnlinkedReason;
};

/** `fact`: a fact whose object is another entity. `link`: a document entity linked to a table row. `foreign_key`: table to table. */
export type AtlasLinkKind = "fact" | "link" | "foreign_key";
/** `from` is the id of the source end of a directed link (a fact points from its subject, a foreign key from the referencing table). */
export type AtlasLink = { a: string; b: string; kind: AtlasLinkKind; weight: number; from?: string };

export type AtlasVersion = { n: number; asOf: string };

/** A side the adapter could not read: the atlas still shows the rest and says which part is missing. */
export type AtlasPartial = "entities" | "files" | "links" | "tables" | "relations";

export type AtlasData = {
  groups: AtlasGroup[];
  members: AtlasMember[];
  links: AtlasLink[];
  versions: AtlasVersion[];
  partial: AtlasPartial[];
};

export const emptyAtlas = (): AtlasData => ({ groups: [], members: [], links: [], versions: [], partial: [] });

export type AtlasMode = "type" | "source";

/** What the scene is told to change; the event reducer and the data diff both produce these. */
export type AtlasOp =
  | { type: "bud"; member: AtlasMember; group?: AtlasGroup }
  | { type: "pulse"; id: string }
  | { type: "retract"; id: string }
  | { type: "breathe" }
  | { type: "refresh" }
  | { type: "link"; link: AtlasLink };
