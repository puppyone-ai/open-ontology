import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type Section = "home" | "sources" | "ontology" | "agents" | "settings";
export type OntologyTab = "entities" | "atlas" | "query" | "files" | "review" | "conflicts";
