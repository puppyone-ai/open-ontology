import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const locales = path.resolve(import.meta.dirname, "..", "..", "..", "src", "locales");
const read = (...parts) => JSON.parse(fs.readFileSync(path.join(locales, ...parts), "utf8"));
const recorded = read("shared-vocabulary.json").entries;
const messageOf = (locale, id) => { const [namespace, ...rest] = id.split("."); return read(locale, `${namespace}.json`)[rest.join(".")]; };

describe("vocabulary shared with the sibling desktop app", () => {
  it("lists a substantial set of messages", () => {
    expect(Object.keys(recorded).length).toBeGreaterThan(200);
  });

  it("covers the everyday words in most languages", () => {
    for (const english of ["Cancel", "Close", "Copy", "Copied", "Save", "Back", "General", "Try again", "Remove", "Name", "Status", "Files", "Home"]) {
      const entry = Object.entries(recorded).find(([, value]) => value.en === english);
      expect(entry, english).toBeDefined();
      expect(Object.keys(entry[1]).filter((key) => key !== "en").length, english).toBeGreaterThanOrEqual(3);
    }
  });

  it("still equals what this app shows, message by message", () => {
    for (const [id, entry] of Object.entries(recorded)) {
      expect(messageOf("en", id), `${id} (en)`).toBe(entry.en);
      for (const [locale, value] of Object.entries(entry)) {
        if (locale === "en") continue;
        expect(messageOf(locale, id), `${id} (${locale})`).toBe(value);
      }
    }
  });
});
