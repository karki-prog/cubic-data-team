"use client";

import { SessionBar } from "@/components/dashboard/SessionBar";

/**
 * Admin chrome — the dashboard's shape (title bar on top, content below) minus
 * the marketing footer. Operator surface, so nothing competes with the working
 * area underneath.
 */
export function AdminShell({
  title = "Admin View",
  subtitle,
  children,
}: {
  title?: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <main className="admin-root">
      <header className="admin-topbar">
        <div className="admin-topbar-inner">
          <div className="admin-topbar-copy">
            <a className="admin-brand" href="/admin">
              {title}
            </a>
            {subtitle ? <p className="admin-sub">{subtitle}</p> : null}
          </div>
          <div className="admin-topbar-auth">
            <a className="admin-exit" href="/">
              Dashboard
            </a>
            <SessionBar />
          </div>
        </div>
        <div className="admin-rule" />
      </header>

      <div className="admin-main">{children}</div>
    </main>
  );
}
