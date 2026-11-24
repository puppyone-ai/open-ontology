import { catalog } from "../../../support/locales";
import fs from "node:fs";
import path from "node:path";
import { IntlMessageFormat } from "intl-messageformat";
import { describe, expect, it } from "vitest";
import {
  dataFlowInputFromState, describeDataFlow, type DataFlow, type DataFlowInput,
} from "@so-model/superDriveDataFlow";
import { makeDrive, makeSnapshot } from "../../../support/fakeApi";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];

const render = (flow: DataFlow, locale: string) => {
  const messages = catalog(locale);
  const say = (messageId: string, values: Record<string, string>) => new IntlMessageFormat(messages[messageId.replace(/^super-drive\./u, "")], locale).format(values) as string;
  return {
    summary: flow.summary
      .map((sentence) => say(sentence.messageId, sentence.values))
      .reduce((first, second) => new IntlMessageFormat(messages["dataFlow.sentences"], locale).format({ first, second }) as string),
    destinations: flow.destinations.map((destination) => say(destination.messageId, destination.values)),
  };
};

const FS = { backend: "fs", modelEnabled: false };
const drivesCases: Array<[string, DataFlowInput["drives"]]> = [
  ["no Drive yet", []],
  ["a local folder", [FS]],
  ["two local folders", [FS, FS]],
  ["one bucket", [{ backend: "s3", bucket: "team-docs" }]],
  ["two buckets of different providers", [{ backend: "s3", bucket: "team-docs" }, { backend: "oss", bucket: "archive" }]],
  ["the same bucket twice", [{ backend: "s3", bucket: "team-docs" }, { backend: "s3", bucket: "team-docs" }]],
  ["a local folder and a bucket", [FS, { backend: "r2", bucket: "shared" }]],
];
const modelCases: Array<[string, DataFlowInput["model"], boolean]> = [
  ["model unknown", null, false],
  ["no model configured", { configured: false }, true],
  ["model configured with provider and region", { configured: true, provider: "Acme AI", region: "eu-west" }, true],
  ["model configured with provider only", { configured: true, provider: "Acme AI" }, true],
  ["model configured, provider not known", { configured: true }, true],
];
describe("data flow: every combination tells the truth (D79)", () => {
  for (const [driveName, drives] of drivesCases) {
    for (const [modelName, model, modelSwitchOn] of modelCases) {
      // A model only sees data when a Drive lets it; test both settings of the per-Drive switch.
      for (const driveSwitch of modelSwitchOn ? [false, true] : [false]) {
        const input: DataFlowInput = { drives: drives.map((drive) => ({ ...drive, modelEnabled: driveSwitch })), model };
        it(`${driveName} / ${modelName}${driveSwitch ? " enabled on the Drive" : ""}`, () => {
          const flow = describeDataFlow(input);
          const { facts } = flow;
          const expectedBuckets = [...new Set(drives.filter((drive) => drive.backend !== "fs").map((drive) => drive.bucket))];
          expect(facts.buckets).toEqual(expectedBuckets);
          const modelReceives = model?.configured === true && drives.length > 0 && driveSwitch;
          expect(facts.modelReceivesExcerpts).toBe(modelReceives);
          expect(flow.summary.some((sentence) => sentence.messageId.includes("cloud"))).toBe(false);

          for (const locale of LOCALES) {
            const text = render(flow, locale);
            // Every bucket the data lives in is named, in the summary and in the full list.
            for (const bucket of expectedBuckets) {
              expect(text.summary, `${locale} summary names ${bucket}`).toContain(bucket as string);
              expect(text.destinations.some((line) => line.includes(bucket as string)), `${locale} list names ${bucket}`).toBe(true);
            }
            // The model is mentioned in the summary exactly when it receives data, and named when known.
            const modelSentences = flow.summary.filter((sentence) => sentence.messageId.includes("summary.model"));
            expect(modelSentences.length === 1, `${locale} model sentence`).toBe(modelReceives);
            if (modelReceives && model?.provider) expect(text.summary).toContain(model.region ? `${model.provider} \u00b7 ${model.region}` : model.provider);
            expect(flow.destinations.filter((destination) => destination.kind === "model").map((destination) => destination.messageId.includes("None"))).toEqual([!modelReceives]);
            // The "stays on this computer" line is only said when no bucket holds the files.
            expect(flow.summary.some((sentence) => sentence.messageId.includes("common.dataLocal"))).toBe(expectedBuckets.length === 0);
            // Results are always local, and files in local folders are listed when there are any or nothing is registered yet.
            expect(flow.destinations.some((destination) => destination.messageId.endsWith("destination.results"))).toBe(true);
            expect(flow.destinations.some((destination) => destination.kind === "filesLocal")).toBe(facts.localFolders > 0 || drives.length === 0);
          }
        });
      }
    }
  }

  it("keeps the default sentence for a local folder in every locale and says nothing about a hosted service", () => {
    const flow = describeDataFlow({ drives: [FS], model: { configured: false } });
    expect(flow.summary.map((sentence) => sentence.messageId)).toEqual(["super-drive.common.dataLocal"]);
    for (const locale of LOCALES) expect(render(flow, locale).summary.length).toBeGreaterThan(10);
    expect(render(flow, "en").summary).toMatch(/stay on this computer/u);
  });

  it("names the provider and region when a model receives excerpts (en)", () => {
    const flow = describeDataFlow({ drives: [{ backend: "fs", modelEnabled: true }], model: { configured: true, provider: "Acme AI", region: "eu-west" } });
    expect(render(flow, "en").summary).toContain("Acme AI \u00b7 eu-west");
  });
});

describe("data flow input from the renderer state", () => {
  const withModel = (status?: string) => makeSnapshot({ health: status ? { components: { model: { status } } } : null });

  it("reads buckets from the Drive config and the per-Drive model switch", () => {
    const input = dataFlowInputFromState([
      makeDrive({ backend: "s3", config: { bucket: "team-docs" }, model_enabled: true }),
      makeDrive({ id: "00000000-0000-4000-8000-000000000002", backend: "fs", config: { root: "/x" } }),
    ], withModel("ok"));
    expect(input.drives).toEqual([{ backend: "s3", bucket: "team-docs", modelEnabled: true }, { backend: "fs", bucket: null, modelEnabled: false }]);
  });

  it("treats a model as configured for every health status except not_configured", () => {
    for (const status of ["ok", "degraded", "unavailable", "limited", "blocked"]) expect(dataFlowInputFromState([], withModel(status)).model).toEqual({ configured: true });
    expect(dataFlowInputFromState([], withModel("not_configured")).model).toEqual({ configured: false });
    expect(dataFlowInputFromState([], withModel("starting")).model).toEqual({ configured: false });
    expect(dataFlowInputFromState([], withModel()).model).toBeNull();
    expect(dataFlowInputFromState([], null).model).toBeNull();
  });
});
