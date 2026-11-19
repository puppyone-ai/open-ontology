import { catalog } from "../../../support/locales";
import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { SO_ERROR_CODES } from "@so-shared/errors.mjs";
import { DOCS_ERROR_CODES } from "@so-contract/docs-api.generated";
import {
  errorFamily, errorMessageIds, isEngineUnavailableError, isOutcomeUncertain, resolveErrorMessageId, toSuperDriveError,
} from "@so-model/superDriveErrors";
import { hasSuperDriveMessage } from "@so-model/superDriveMessageKeys";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];


describe("every code has its own message (20-frontend §4.8)", () => {
  it("has errors.<CODE> in all 8 locales for every code of errors.v1.json and every main-process code that reaches the UI", () => {
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const { code } of DOCS_ERROR_CODES) expect(messages[`errors.${code}`], `${locale}:${code}`).toBeTruthy();
    }
  });

  it("never shows a raw engine key or code as the message, and translations differ from the key", () => {
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const { code } of DOCS_ERROR_CODES) {
        const message = messages[`errors.${code}`];
        expect(message, `${locale}:${code}`).not.toBe(code);
        expect(message, `${locale}:${code}`).not.toMatch(/^super-drive\./u);
      }
    }
  });

  it("keeps placeholders identical across locales", () => {
    const names = (message: string) => [...message.matchAll(/\{(\w+)/gu)].map((match) => match[1]).sort().join(",");
    const english = catalog("en");
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const { code } of DOCS_ERROR_CODES) expect(names(messages[`errors.${code}`]), `${locale}:${code}`).toBe(names(english[`errors.${code}`]));
    }
  });

  it("covers the upload and engine codes of the main process", () => {
    for (const code of SO_ERROR_CODES.filter((entry) => entry.startsWith("SO_UPLOAD_"))) {
      for (const locale of LOCALES) expect(catalog(locale)[`errors.${code}`], `${locale}:${code}`).toBeTruthy();
    }
  });
});

describe("error code to message", () => {
  it("every engine code resolves to a message that exists (specific, family or generic)", () => {
    for (const { code } of DOCS_ERROR_CODES) {
      const id = resolveErrorMessageId(code, hasSuperDriveMessage);
      expect(hasSuperDriveMessage(id), code).toBe(true);
    }
  });

  it("every main-process code resolves to a message that exists", () => {
    for (const code of SO_ERROR_CODES) expect(hasSuperDriveMessage(resolveErrorMessageId(code, hasSuperDriveMessage)), code).toBe(true);
  });

  it("falls back from the code to its family to the generic message", () => {
    expect(errorMessageIds("DOCS_PARSE_CORRUPT")).toEqual([
      "super-drive.errors.DOCS_PARSE_CORRUPT", "super-drive.errors.family.PARSE", "super-drive.errors.unknown",
    ]);
    expect(errorMessageIds("SO_TIMEOUT")).toEqual(["super-drive.errors.SO_TIMEOUT", "super-drive.errors.unknown"]);
    expect(resolveErrorMessageId("DOCS_SHARE_TARGET_OCCUPIED", (id) => id === "super-drive.errors.family.SHARE")).toBe("super-drive.errors.family.SHARE");
    expect(resolveErrorMessageId("DOCS_SHARE_TARGET_OCCUPIED", () => false)).toBe("super-drive.errors.unknown");
    expect(resolveErrorMessageId("DOCS_SHARE_TARGET_OCCUPIED", (id) => id === "super-drive.errors.DOCS_SHARE_TARGET_OCCUPIED")).toBe("super-drive.errors.DOCS_SHARE_TARGET_OCCUPIED");
  });

  it("every engine family has a family message", () => {
    for (const { code } of DOCS_ERROR_CODES) {
      const family = errorFamily(code);
      expect(family, code).not.toBeNull();
      expect(hasSuperDriveMessage(`super-drive.errors.family.${family}`)).toBe(true);
    }
    expect(errorFamily("SO_TIMEOUT")).toBeNull();
    expect(errorFamily("DOCS_NOPE_X")).toBeNull();
  });

  it("copies the envelope without sharing the details object", () => {
    const failure = { ok: false as const, error: { code: "X", retryable: true, details: { a: 1 }, httpStatus: 412 } };
    const error = toSuperDriveError(failure);
    expect(error).toEqual(failure.error);
    expect(error.details).not.toBe(failure.error.details);
    expect(toSuperDriveError({ ok: false, error: { code: "Y", retryable: false, details: {} } })).not.toHaveProperty("httpStatus");
  });

  it("treats a timed-out write as uncertain and engine-down errors as unavailability", () => {
    const timeout = { code: "SO_TIMEOUT", retryable: true, details: {} };
    expect(isOutcomeUncertain(timeout, true)).toBe(true);
    expect(isOutcomeUncertain(timeout, false)).toBe(false);
    expect(isOutcomeUncertain({ ...timeout, code: "DOCS_ENGINE_INTERNAL" }, true)).toBe(false);
    expect(isEngineUnavailableError({ code: "SO_ENGINE_UNREACHABLE", retryable: true, details: {} })).toBe(true);
    expect(isEngineUnavailableError({ code: "DOCS_PARSE_CORRUPT", retryable: false, details: {} })).toBe(false);
  });
});
