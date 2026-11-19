/** The atlas data model (D88, 25 section 7.3): what the adapter produces and the scene consumes. Plain data, no engine types. */

/** Low-saturation tints of the ink-blue accent and the status colours; the renderer maps them to concrete colours. */
export type AtlasTone = "ink" | "moss" | "sand" | "clay" | "stone";

/** `type`: an entity type (members are entities). `drive`: a document source (members are files). `db`: a database (members are tables). */
export type AtlasGroupKind = "type" | "drive" | "db";

export type AtlasRef =
  | { kind: "entity"; key: string }
  | { kind: "file"; fileId: string; path: string }
  | { kind: "table"; table: string };

export type AtlasGroup = { id: string; kind: AtlasGroupKind; label: string; tone: AtlasTone };

export type AtlasMember = {
  id: string;
  group: string;
  label: string;
  ref: AtlasRef;
  /** Facts (entities), versions or rows (tables): sizes the cell, never the colour. */
  weight: number;
  /** The ontology version in which it first appeared, when the engine says; `null` otherwise. */
  born: number | null;
};

/** `fact`: a fact whose object is another entity. `link`: a document entity linked to a table row. `foreign_key`: table to table. */
export type AtlasLinkKind = "fact" | "link" | "foreign_key";
export type AtlasLink = { a: string; b: string; kind: AtlasLinkKind; weight: number };

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
