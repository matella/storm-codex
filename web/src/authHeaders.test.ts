import { afterEach, describe, expect, it, vi } from "vitest";
import { authHeaders } from "./api";
import { castScene, fetchCast } from "./castApi";

function stubToken(token: string | null) {
  vi.stubGlobal("localStorage", { getItem: () => token });
}

describe("authHeaders", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("ajoute le Bearer quand un token admin est enregistré", () => {
    stubToken("s3cret");
    expect(authHeaders()).toEqual({ Authorization: "Bearer s3cret" });
  });

  it("n'ajoute rien sans token ou sans stockage", () => {
    stubToken(null);
    expect(authHeaders()).toEqual({});
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("bloqué"); } });
    expect(authHeaders()).toEqual({});
  });

  it("les écritures de la régie envoient le token, pas les lectures", async () => {
    stubToken("s3cret");
    const calls: RequestInit[] = [];
    vi.stubGlobal("fetch", async (_: string, init: RequestInit) => {
      calls.push(init);
      return new Response("{}", { status: 200 });
    });
    await castScene("hide");
    await fetchCast();
    expect((calls[0].headers as Record<string, string>).Authorization).toBe("Bearer s3cret");
    expect((calls[1].headers as Record<string, string>).Authorization).toBeUndefined();
  });
});
