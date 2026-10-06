"use client";

import { useEffect, useState } from "react";

type State = "checking" | "superadmin" | "denied";

/**
 * Superadmin-only gate for /admin. Wrap *inside* AuthGate: AuthGate establishes
 * that someone is signed in, this decides whether that someone is a superadmin.
 *
 * Note this is stricter than the tracker's "staff" check — being on the cubicit
 * domain is not enough, the address must be on the superadmin allow-list.
 *
 * Anyone signed in but not allowed gets a plain refusal rather than a bounce to
 * /login — sending them to a login they are already past is a loop, not an
 * answer.
 */
export function SuperadminGate({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<State>("checking");

  useEffect(() => {
    let cancelled = false;
    fetch("/api/admin/me", { credentials: "same-origin" })
      .then((res) => res.json())
      .then((data: { superadmin?: boolean; signedIn?: boolean }) => {
        if (cancelled) return;
        if (data?.superadmin) {
          setState("superadmin");
          return;
        }
        if (data?.signedIn === false) {
          window.location.replace(`/login?next=${encodeURIComponent("/admin")}`);
          return;
        }
        setState("denied");
      })
      .catch(() => {
        if (!cancelled) setState("denied");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (state === "checking") {
    return <main className="admin-gate-blank" />;
  }

  if (state === "denied") {
    return (
      <main className="admin-denied">
        <div className="admin-denied-card">
          <h1>Superadmin only</h1>
          <p>
            This page is limited to superadmins. You are signed in, but your account does not have
            superadmin access.
          </p>
          <a className="admin-denied-link" href="/">
            Back to dashboard
          </a>
        </div>
      </main>
    );
  }

  return <>{children}</>;
}
