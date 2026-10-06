"use client";

import { useEffect } from "react";

const CLIENT_VERSION = process.env.NEXT_PUBLIC_APP_VERSION || "dev";
const CHECK_MS = 30000;

/**
 * After a deploy the HTML document is not cached, but an already-open tab
 * still runs the previous bundle. Compare the baked-in build id with
 * /api/version and reload once when they differ.
 */
export function BuildRefresh() {
  useEffect(() => {
    if (!CLIENT_VERSION || CLIENT_VERSION === "dev") return;
    let cancelled = false;

    const check = async () => {
      try {
        const res = await fetch("/api/version", { cache: "no-store", credentials: "same-origin" });
        const json = (await res.json()) as { ok?: boolean; version?: string };
        if (cancelled || !json?.version || json.version === CLIENT_VERSION) return;
        const key = `cubic:reloaded:${json.version}`;
        if (sessionStorage.getItem(key)) return;
        sessionStorage.setItem(key, "1");
        // Clear this site's cached files (not cookies) so the reload gets the new build everywhere.
        try {
          await fetch("/api/version?clear=1", { cache: "no-store", credentials: "same-origin" });
        } catch {
          /* still reload below */
        }
        window.location.reload();
      } catch {
        /* offline / mid-deploy — try again on the next interval */
      }
    };

    void check();
    const timer = window.setInterval(() => void check(), CHECK_MS);
    const onVisible = () => {
      if (document.visibilityState === "visible") void check();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  return null;
}
