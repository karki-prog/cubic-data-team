"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { PageSkeleton } from "@/components/ui/Skeleton";

function normalizePath(pathname: string) {
  const p = pathname.replace(/\/+$/, "");
  return p || "/";
}

function isPublicPath(pathname: string) {
  const p = normalizePath(pathname);
  return p === "/" || p === "/login" || p.startsWith("/api");
}

export function AuthGate({
  children,
  fallback,
}: {
  children: React.ReactNode;
  /** Page-shaped skeleton shown while the session is checked (defaults to a generic one). */
  fallback?: React.ReactNode;
}) {
  const pathname = usePathname() || "/";
  // Start as ready so SSR HTML matches the first client paint (avoids hydration
  // errors on 404 / /api/*). Session is checked after mount.
  const [ready, setReady] = useState(true);

  useEffect(() => {
    if (isPublicPath(pathname)) {
      setReady(true);
      return;
    }
    let cancelled = false;
    setReady(false);
    fetch("/api/auth/me", { credentials: "same-origin" })
      .then((res) => res.json())
      .then((data: { ok?: boolean; user?: unknown }) => {
        if (cancelled) return;
        if (data?.user) {
          setReady(true);
          return;
        }
        const next = encodeURIComponent(pathname || "/");
        window.location.replace(`/login?next=${next}`);
      })
      .catch(() => {
        if (!cancelled) {
          const next = encodeURIComponent(pathname || "/");
          window.location.replace(`/login?next=${next}`);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [pathname]);

  if (!ready) {
    return <>{fallback ?? <PageSkeleton />}</>;
  }
  return <>{children}</>;
}
