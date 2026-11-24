import { catalog } from "../../../support/locales";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ENGINE_STATUSES } from "@so-shared/engine-contract.mjs";
import {
  COMPONENT_NAMES, COMPONENT_STATUS_VALUES, ENGINE_STATUS_ORDER, degradedComponents, isEngineReady, presentEngine,
} from "@so-model/superDriveStateModel";
import { makeSnapshot } from "../../../support/fakeApi";

const LOCALES = ["en", "es", "pt-BR", "fr", "de", "ja", "ko", "zh-Hans"];


describe("engine state model (20-frontend §4.4)", () => {
  it("covers exactly the engine statuses the main process can report", () => {
    expect([...ENGINE_STATUS_ORDER]).toEqual([...ENGINE_STATUSES]);
    for (const status of ENGINE_STATUSES) expect(presentEngine(makeSnapshot({ status })).status).toBe(status);
  });

  it("has a full message, a pill text and an action story for every status in every locale", () => {
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const status of ENGINE_STATUSES) {
        const presentation = presentEngine(makeSnapshot({ status, reason: status === "restarting" ? "process_exit" : null }));
        const strip = (id: string) => id.replace(/^super-drive\./u, "");
        expect(messages[strip(presentation.messageId)], `${locale}:${presentation.messageId}`).toBeTruthy();
        expect(messages[strip(presentation.pillId)], `${locale}:${presentation.pillId}`).toBeTruthy();
        for (const action of [presentation.primary, ...presentation.secondary]) {
          if (action) expect(messages[`engine.action.${action}`], `${locale}:${action}`).toBeTruthy();
        }
      }
    }
  });

  it("gives every state that can be left by the user a way forward, and never two primary actions", () => {
    const withoutAction = ENGINE_STATUS_ORDER.filter((status) => presentEngine(makeSnapshot({ status })).primary === null && presentEngine(makeSnapshot({ status })).secondary.length === 0);
    // Transient or terminal-by-design states only: nothing to press.
    expect(withoutAction.sort()).toEqual(["probing", "stopping", "unsupported_platform"].sort());
  });

  it("refines restarting by reason and keeps the drive-change tone calm", () => {
    expect(presentEngine(makeSnapshot({ status: "restarting", reason: "drive_change" })).messageId).toBe("super-drive.engine.status.applyingDriveChange");
    expect(presentEngine(makeSnapshot({ status: "restarting", reason: "drive_change" })).tone).toBe("info");
    expect(presentEngine(makeSnapshot({ status: "restarting", reason: "engine_container" })).messageId).toBe("super-drive.engine.status.restartingContainer");
    const exit = presentEngine(makeSnapshot({ status: "restarting", reason: "process_exit", restarts: { count: 3, nextAttemptAt: new Date(Date.now() + 15_000).toISOString() } }));
    expect(exit.messageId).toBe("super-drive.engine.status.restarting");
    expect(exit.values.attempt).toBe(3);
    expect(Number(exit.values.seconds)).toBeGreaterThanOrEqual(14);
  });

  it("treats only ok-or-not-yet-relevant components as healthy (an unconfigured model is not degradation)", () => {
    const health = (components: Record<string, string>) => ({ components: Object.fromEntries(Object.entries(components).map(([name, status]) => [name, { status }])) });
    expect(degradedComponents(health({ db: "ok", parser: "ok", model: "not_configured", cas: "ok", engine: "ok" }))).toEqual([]);
    expect(degradedComponents(health({ db: "degraded", model: "limited", cas: "blocked", parser: "unavailable" }))).toEqual(["db", "parser", "model", "cas"]);
    expect(degradedComponents(null)).toEqual([]);
    expect(isEngineReady(makeSnapshot({ status: "degraded" }))).toBe(true);
    expect(isEngineReady(makeSnapshot({ status: "restarting" }))).toBe(false);
    expect(isEngineReady(null)).toBe(false);
  });

  it("every component and status has text in every locale", () => {
    for (const locale of LOCALES) {
      const messages = catalog(locale);
      for (const name of COMPONENT_NAMES) expect(messages[`engine.component.${name}.label`], `${locale}:${name}`).toBeTruthy();
      for (const status of COMPONENT_STATUS_VALUES) expect(messages[`engine.component.status.${status}`], `${locale}:${status}`).toBeTruthy();
    }
  });
});
