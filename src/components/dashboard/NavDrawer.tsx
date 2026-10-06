"use client";

import { useEffect, useId, useState } from "react";
import { SessionBar } from "./SessionBar";

export function NavDrawer() {
  const [open, setOpen] = useState(false);
  const titleId = useId();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", onKey);
    return () => {
      document.body.style.overflow = prev;
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    const mq = window.matchMedia("(min-width: 1024px)");
    const onChange = () => {
      if (mq.matches) setOpen(false);
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return (
    <>
      <button
        type="button"
        className="cubic-dash-menu"
        aria-label="Open menu"
        aria-expanded={open}
        aria-controls="cubic-nav-drawer"
        onClick={() => setOpen(true)}
      >
        <span className="cubic-dash-menu-bars" aria-hidden>
          <span />
          <span />
          <span />
        </span>
      </button>

      {open ? (
        <div className="cubic-drawer" role="presentation">
          <button
            type="button"
            className="cubic-drawer-backdrop"
            aria-label="Close menu"
            onClick={() => setOpen(false)}
          />
          <aside
            id="cubic-nav-drawer"
            className="cubic-drawer-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
          >
            <div className="cubic-drawer-head">
              <p id={titleId} className="cubic-drawer-brand">
                CUBIC
              </p>
              <button
                type="button"
                className="cubic-drawer-close"
                aria-label="Close menu"
                onClick={() => setOpen(false)}
              >
                <svg width="16" height="16" viewBox="0 0 14 14" fill="none" aria-hidden>
                  <path
                    d="M3 3l8 8M11 3L3 11"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                  />
                </svg>
              </button>
            </div>
            <div className="cubic-drawer-account">
              <SessionBar variant="drawer" />
            </div>
          </aside>
        </div>
      ) : null}
    </>
  );
}
