import { useEffect, useState } from "react";
import { invoke } from "../../model/bridgeInvoke";
import { placeholderLetter } from "../../model/details/mediaModel";

/** Names of the people the ontology already knows, for the name field of a speaker (a placeholder such as "Speaker A" is not a person). */
export function usePersonNames(enabled: boolean): string[] {
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => {
    if (!enabled) return undefined;
    let live = true;
    void invoke("entities-search", { q: "", type: "Person", limit: 100 }).then((result) => {
      if (!live || !result.ok) return;
      const seen = new Set<string>();
      for (const entity of result.value.items) {
        const name = typeof entity.name === "string" ? entity.name.trim() : "";
        if (name !== "" && placeholderLetter(name) === null) seen.add(name);
      }
      setNames([...seen].sort((a, b) => a.localeCompare(b)));
    });
    return () => { live = false; };
  }, [enabled]);
  return names;
}
