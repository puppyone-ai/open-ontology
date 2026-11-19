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
  it("rejects table entries that could carry shell or SQL syntax", () => {
    expect(validateForm({ ...form, include: "public.a; drop" })).toEqual(["include"]);
    expect(validateForm({ ...form, include: "public.a,public.b" })).toEqual([]);
  });
});
