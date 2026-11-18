// @vitest-environment jsdom
// The app's real route table, mounted: the home page lists requests nobody has taken apart from the drafts in the works.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFrameMode } from "@ferrum/records/frame";
import { CONTRACTS_PACKAGES, routes } from "./routes";

const me = {
  accounts: null, default_company: null, display_timezone: "UTC", fiscal_year_start_month: 1, packages: ["core", "sign", "contracts"], scopes: {},
  shell_v2: true, tenant: "t1", timezone: "UTC", today: "2026-11-18", two_factor_setup: false,
  user: { email: "cm@example.test", full_name: "Casey Manager", id: "u1", is_admin: false, roles: ["Contract Manager"] },
};
const contracts = [
  { id: "c1", title: "NDA for the demo", status: "Draft", requested_by: "asker@example.test", owner: null, counterparty_name: "Initech" },
  { id: "c2", title: "Support plan", status: "Draft", owner: "cm@example.test", counterparty_name: "Globex" },
];
const obligations = [
  { id: "o1", description: "Quarterly service review", status: "Open", due_on: "2026-12-01", repeat_months: 3 },
  { id: "o2", description: "Deliver the audit report", status: "Open", due_on: "2026-12-02", repeat_months: 0 },
];

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const path = new URL(String(input instanceof Request ? input.url : input), "http://localhost").pathname;
      const json = (b: unknown, status = 200) => new Response(JSON.stringify(b), { status, headers: { "content-type": "application/json" } });
      if (path === "/api/auth/me") return json(me);
      if (path === "/api/meta") return json({ entities: [], modules: [] });
      if (path === "/api/v1/Contract") return json({ rows: contracts, total: contracts.length });
      if (path === "/api/v1/Obligation") return json({ rows: obligations, total: obligations.length });
      if (path.startsWith("/api/v1/")) return json({ rows: [], total: 0 });
      return json({ error: "not found" }, 404);
    }),
  );
  setFrameMode("app", CONTRACTS_PACKAGES);
});
afterEach(() => vi.unstubAllGlobals());

/** The text of each queue on the page, by its title. */
async function queues(path: string): Promise<Record<string, string>> {
  const host = document.createElement("div");
  document.body.append(host);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
  });
  for (let i = 0; i < 40 && !host.querySelector("section.queue"); i++) await act(async () => void (await new Promise((r) => setTimeout(r, 25))));
  const out: Record<string, string> = {};
  for (const s of host.querySelectorAll("section.queue")) out[s.querySelector("h2")?.textContent ?? ""] = s.textContent ?? "";
  await act(async () => root.unmount());
  host.remove();
  return out;
}

describe("the home page", () => {
  it("lists requests apart from drafts", async () => {
    const q = await queues("/");
    expect(q["Requests"]).toContain("NDA for the demo");
    expect(q["Requests"]).toContain("asker@example.test");
    expect(q["Requests"]).not.toContain("Support plan");
    expect(q["In the works"]).toContain("Support plan");
    expect(q["In the works"]).not.toContain("NDA for the demo");
  });

  it("marks a repeating obligation", async () => {
    const q = await queues("/");
    expect(q["Obligations"]).toMatch(/Quarterly service review.*Repeats/);
    expect(q["Obligations"]).not.toMatch(/Deliver the audit report.*Repeats/);
  });
});
