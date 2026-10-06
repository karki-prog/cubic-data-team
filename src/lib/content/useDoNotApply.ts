"use client";

import { useEffect, useState } from "react";

export type DoNotApplyCompany = {
  name: string;
  reason: string;
};

/** One row per company name (case-insensitive). First non-empty reason wins. */
export function uniqueDoNotApplyCompanies(list: DoNotApplyCompany[]): DoNotApplyCompany[] {
  const seen = new Map<string, DoNotApplyCompany>();
  for (const row of list) {
    const name = row.name.trim();
    if (!name) continue;
    const key = name.toLowerCase();
    const reason = (row.reason || "").trim();
    const existing = seen.get(key);
    if (!existing) {
      seen.set(key, { name, reason });
      continue;
    }
    if (!existing.reason && reason) {
      existing.reason = reason;
    }
  }
  return [...seen.values()].sort((a, b) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
  );
}

export function useDoNotApplyCompanies(enabled = true) {
  const [companies, setCompanies] = useState<DoNotApplyCompany[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const load = (silent = false) => {
      if (!silent) {
        setLoading(true);
        setError("");
      }
      fetch("/api/do-not-apply", { cache: "no-store", credentials: "same-origin" })
        .then(async (res) => {
          const json = (await res.json()) as {
            ok?: boolean;
            error?: string;
            companies?: DoNotApplyCompany[];
          };
          if (cancelled) return;
          if (!res.ok || !json.ok) {
            if (!silent) setError(json.error || "Could not load Do Not Apply list.");
            return;
          }
          setCompanies(uniqueDoNotApplyCompanies(json.companies || []));
          setError("");
        })
        .catch((err: unknown) => {
          if (!cancelled && !silent) {
            setError(err instanceof Error ? err.message : "Could not load Do Not Apply list.");
          }
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    };
    load(false);
    const timer = window.setInterval(() => load(true), 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled]);

  return { companies, loading, error };
}
