// Contracts' route table, apart from the page that mounts it so a test can mount the real thing.

import { useEffect } from "react";
import { Link, type RouteObject, useRouteError } from "react-router";
import { shell, t } from "@ferrum/client";
import { EmptyState, ErrorState } from "@ferrum/kit";
import { RequireAuth } from "@ferrum/records/frame";

/** A page of the records frame, loaded when first opened. */
const page = (name: "FormView" | "ListView" | "ReportView") => () => import("./records").then((m) => ({ Component: m[name] }));

/** Contracts' package: the sidebar shows its module. */
export const CONTRACTS_PACKAGES = ["contracts"];

function RouteError() {
  return <ErrorState error={useRouteError()} onRetry={() => window.location.reload()} />;
}

function NotFound() {
  return (
    <EmptyState title={t("Page not found")}>
      <Link to="/">{t("Go to the contracts")}</Link>
    </EmptyState>
  );
}

/** Every page tells the shell it started. */
function Started() {
  useEffect(() => shell().ready(), []);
  return null;
}

export const routes: RouteObject[] = [
  {
    path: "/",
    element: (
      <>
        <Started />
        <RequireAuth />
      </>
    ),
    errorElement: <RouteError />,
    children: [
      { index: true, lazy: () => import("./Home").then((m) => ({ Component: m.Home })) },
      { path: "e/:entity", lazy: page("ListView") },
      { path: "e/:entity/:id", lazy: page("FormView") },
      { path: "r/:name", lazy: page("ReportView") },
      { path: "*", element: <NotFound /> },
    ],
  },
];
