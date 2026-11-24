import { describe, expect, it } from "vitest";
import { validateForm } from "../../../src/renderer/screens/sources/AddDatabase";

const form = { kind: "postgres", name: "shop", host: "db.example.test", port: "", database: "shop", user: "ro", password: "p", caMode: "paste" as const, caPem: "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----", caGrant: null, include: "public.customers\npublic.orders", exclude: "" };

describe("Add database form", () => {
  it("accepts a complete form", () => expect(validateForm(form)).toEqual([]));
  it("names every missing or invalid field", () => {
    expect(validateForm({ ...form, name: "bad name", host: "", port: "70000", database: " ", user: "", password: "", caPem: "", include: "" }).sort())
      .toEqual(["ca", "database", "host", "include", "name", "password", "port", "user"]);
  });
  it("needs a certificate authority only for kinds that require one", () => {
    expect(validateForm({ ...form, caPem: "" })).toEqual(["ca"]);
    expect(validateForm({ ...form, kind: "oracle", caPem: "" })).toEqual([]);
    expect(validateForm({ ...form, kind: "kingbase", caPem: "", caMode: "file", caGrant: { id: "grant-123", path: "/x/ca.pem" } })).toEqual([]);
  });
  it("follows the CLI's trust-anchor rule for kinds that take plain text on this machine only", () => {
    const doc = { ...form, kind: "mongodb", caPem: "", include: "shop.customers" };
    expect(validateForm({ ...doc, host: "127.0.0.1" })).toEqual([]);
    expect(validateForm({ ...doc, host: "localhost" })).toEqual([]);
    expect(validateForm({ ...doc, host: "db.example.test" })).toEqual(["ca"]);
    expect(validateForm({ ...doc, kind: "clickhouse", host: "db.example.test", caPem: form.caPem })).toEqual([]);
  });
  it("never asks for a certificate where the transport owns TLS", () => {
    expect(validateForm({ ...form, kind: "dm8", caPem: "" })).toEqual([]);
    expect(validateForm({ ...form, kind: "oracle", caPem: "" })).toEqual([]);
    expect(validateForm({ ...form, kind: "sqlserver", caPem: "" })).toEqual(["ca"]);
  });
  it("rejects table entries that could carry shell or SQL syntax", () => {
    expect(validateForm({ ...form, include: "public.a; drop" })).toEqual(["include"]);
    expect(validateForm({ ...form, include: "public.a,public.b" })).toEqual([]);
  });
});
