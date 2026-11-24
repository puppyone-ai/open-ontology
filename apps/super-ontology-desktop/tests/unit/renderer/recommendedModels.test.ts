import { describe, expect, it } from "vitest";
import { isModelAddress, RECOMMENDED_MODEL, SUGGESTED_MODELS } from "../../../src/renderer/model/settings/recommendedModels";

describe("the model a new connection starts with", () => {
  it("is the first suggestion and every suggestion is listed once", () => {
    expect(RECOMMENDED_MODEL.trim()).not.toBe("");
    expect(SUGGESTED_MODELS[0]).toBe(RECOMMENDED_MODEL);
    expect(new Set(SUGGESTED_MODELS).size).toBe(SUGGESTED_MODELS.length);
  });
});

describe("the address of a model connection", () => {
  it("is an http(s) address or the absolute path of a file that holds it", () => {
    expect(isModelAddress("https://models.example.test/v1")).toBe(true);
    expect(isModelAddress("file:/Users/someone/.config/endpoint")).toBe(true);
    expect(isModelAddress("file:relative/path")).toBe(false);
    expect(isModelAddress("models.example.test")).toBe(false);
    expect(isModelAddress("https://")).toBe(false);
  });
});
