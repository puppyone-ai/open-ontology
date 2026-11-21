import { describe, expect, it } from "vitest";
import { RECOMMENDED_MODEL, SUGGESTED_MODELS } from "../../../src/renderer/model/settings/recommendedModels";

describe("the model a new connection starts with", () => {
  it("is the first suggestion and every suggestion is listed once", () => {
    expect(RECOMMENDED_MODEL.trim()).not.toBe("");
    expect(SUGGESTED_MODELS[0]).toBe(RECOMMENDED_MODEL);
    expect(new Set(SUGGESTED_MODELS).size).toBe(SUGGESTED_MODELS.length);
  });
});
