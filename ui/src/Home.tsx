// The contracts by stage (in the works, out for signature, active), those whose notice to stop a
// renewal is due, and obligations open, the overdue first. Each opens its record.

import { useQuery } from "@tanstack/react-query";
import { api, docPath, formatValue, t, isolate } from "@ferrum/client";
import { Badge, ButtonLink, HomeSkeleton, LoadFailed, PageHead, Queue, QueueGrid, QueueItem, StatusBadge, useShellTitle } from "@ferrum/kit";

export interface Row {
  id: string;
  [k: string]: unknown;
}

const rows = (entity: string, filters: unknown[], extra: Record<string, string | number> = {}) =>
  api.get<{ rows: Row[] }>(docPath(entity), { filters: JSON.stringify(filters), limit: 500, ...extra }).then((r) => r.rows);

const STAGES: Record<string, string> = {
  get Draft() { return t("In the works"); },
  get "In Review"() { return t("In the works"); },
  get Approved() { return t("In the works"); },
  get "Out for Signature"() { return t("Out for signature"); },
  get Signed() { return t("Active"); },
  get Active() { return t("Active"); },
};

/** Contracts by stage, in the stages' order. */
export function byStage(list: Row[]): [string, Row[]][] {
  const order = [t("In the works"), t("Out for signature"), t("Active")];
  const out = new Map<string, Row[]>(order.map((s) => [s, []]));
  for (const c of list) {
    const stage = STAGES[String(c.status)];
    if (stage) out.get(stage)?.push(c);
  }
  return order.map((s) => [s, out.get(s) ?? []]);
}

/** A day as the person reads it ("20 Nov 2026"); empty when there is none. */
const day = (v: unknown) => formatValue({ type: "date" }, v);

/** The queues show this many rows each; the rest is "and N more", linking to the list. */
const LIMIT = 5;

export function Home() {
  useShellTitle(t("Contracts"));
  const q = useQuery({
    queryKey: ["contracts", "home"],
    queryFn: async () => {
      const [contracts, obligations] = await Promise.all([rows("Contract", [["status", "in", Object.keys(STAGES)]], { sort: "ends_on asc" }), rows("Obligation", [["status", "eq", "Open"]], { sort: "due_on asc" })]);
      return { contracts, obligations };
    },
  });
  const create = (
    <ButtonLink to="/e/Contract/new" variant="primary" icon="plus">
      {t("New contract")}
    </ButtonLink>
  );
  if (q.isPending) return (
    <>
      <PageHead title={t("Contracts")} />
      <HomeSkeleton queues={4} />
    </>
  );
  if (q.isError) return (
    <>
      <PageHead title={t("Contracts")} actions={create} />
      <LoadFailed error={q.error} onRetry={() => void q.refetch()} />
    </>
  );
  const notice = q.data.contracts.filter((c) => c.notice_due);
  // What is late comes before what is merely due.
  const obligations = [...q.data.obligations].sort((a, b) => Number(Boolean(b.overdue)) - Number(Boolean(a.overdue)));
  const late = obligations.some((o) => o.overdue);
  const contract = (c: Row, meta: string) => <QueueItem key={c.id} to={`/e/Contract/${c.id}`} title={isolate(String(c.title))} meta={meta} trailing={<StatusBadge value={c.status} />} />;
  return (
    <>
      <PageHead title={t("Contracts")} description={notice.length + obligations.length > 0 ? t("{notice} notice due · {open} obligations open", { notice: notice.length, open: obligations.length }) : undefined} actions={create} />
      <QueueGrid>
        {notice.length > 0 && (
          <Queue title={t("Notice due before renewal")} count={notice.length} tone="attention" more={{ to: "/e/Contract", count: notice.length - LIMIT }}>
            {notice.slice(0, LIMIT).map((c) => contract(c, t("renews after {day}", { day: day(c.ends_on) })))}
          </Queue>
        )}
        <Queue title={t("Obligations")} count={obligations.length} tone={late ? "attention" : "default"} more={{ to: "/e/Obligation", count: obligations.length - LIMIT }}>
          {obligations.slice(0, LIMIT).map((o) => (
            <QueueItem key={o.id} to={`/e/Obligation/${o.id}`} title={isolate(String(o.description))} meta={t("due {day}", { day: day(o.due_on) })} trailing={o.overdue ? <Badge tone="red">{t("Overdue")}</Badge> : null} />
          ))}
        </Queue>
        {byStage(q.data.contracts).map(([stage, list]) => (
          <Queue key={stage} title={stage} count={list.length} more={{ to: "/e/Contract", count: list.length - LIMIT }}>
            {list.slice(0, LIMIT).map((c) => contract(c, isolate(String(c.counterparty_name ?? ""))))}
          </Queue>
        ))}
      </QueueGrid>
    </>
  );
}
