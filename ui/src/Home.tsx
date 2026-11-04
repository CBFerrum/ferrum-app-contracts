// The contracts by stage (in the works, out for signature, active), those whose notice to stop a
// renewal is due, and obligations open, the overdue first. Each opens its record.

import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { api, docPath } from "@ferrum/client";
import { Badge, ErrorState, PageLoading, useShellTitle } from "@ferrum/kit";

export interface Row {
  id: string;
  [k: string]: unknown;
}

const rows = (entity: string, filters: unknown[], extra: Record<string, string | number> = {}) =>
  api.get<{ rows: Row[] }>(docPath(entity), { filters: JSON.stringify(filters), limit: 500, ...extra }).then((r) => r.rows);

const STAGES: Record<string, string> = {
  Draft: "In the works",
  "In Review": "In the works",
  Approved: "In the works",
  "Out for Signature": "Out for signature",
  Signed: "Active",
  Active: "Active",
};

/** Contracts by stage, in the stages' order. */
export function byStage(list: Row[]): [string, Row[]][] {
  const order = ["In the works", "Out for signature", "Active"];
  const out = new Map<string, Row[]>(order.map((s) => [s, []]));
  for (const c of list) {
    const stage = STAGES[String(c.status)];
    if (stage) out.get(stage)?.push(c);
  }
  return order.map((s) => [s, out.get(s) ?? []]);
}

export function Home() {
  useShellTitle("Contracts");
  const q = useQuery({
    queryKey: ["contracts", "home"],
    queryFn: async () => {
      const [contracts, obligations] = await Promise.all([rows("Contract", [["status", "in", Object.keys(STAGES)]], { sort: "ends_on asc" }), rows("Obligation", [["status", "eq", "Open"]], { sort: "due_on asc" })]);
      return { contracts, obligations };
    },
  });
  if (q.isPending) return <PageLoading />;
  if (q.isError) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  const notice = q.data.contracts.filter((c) => c.notice_due);
  return (
    <div className="np-page">
      <header className="row" style={{ justifyContent: "space-between" }}>
        <h2>Contracts</h2>
        <Link to="/e/Contract/new">New contract</Link>
      </header>
      {notice.length > 0 && (
        <section className="card card-pad stack" aria-label="Notice due">
          <h3>Notice due before renewal ({notice.length})</h3>
          <ul className="stack">
            {notice.map((c) => (
              <li key={c.id}>
                <Link to={`/e/Contract/${c.id}`}>{`${String(c.title)} · renews after ${String(c.ends_on ?? "")}`}</Link>
              </li>
            ))}
          </ul>
        </section>
      )}
      <div className="np-columns">
        {byStage(q.data.contracts).map(([stage, list]) => (
          <section key={stage} className="card card-pad stack" aria-label={stage}>
            <h3>
              {stage} ({list.length})
            </h3>
            <ul className="stack">
              {list.map((c) => (
                <li key={c.id}>
                  <Link to={`/e/Contract/${c.id}`}>{`${String(c.title)} · ${String(c.counterparty_name ?? "")}`}</Link> <Badge tone="neutral">{String(c.status)}</Badge>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <section className="card card-pad stack" aria-label="Obligations">
          <h3>Obligations ({q.data.obligations.length})</h3>
          <ul className="stack">
            {q.data.obligations.map((o) => (
              <li key={o.id}>
                <Link to={`/e/Obligation/${o.id}`}>{`${String(o.description)} · due ${String(o.due_on ?? "")}`}</Link> {o.overdue ? <Badge tone="red">Overdue</Badge> : null}
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
