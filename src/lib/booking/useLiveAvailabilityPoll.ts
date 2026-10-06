"use client";

import { useEffect, useRef } from "react";

type LoadFn = (opts?: { silent?: boolean }) => void | Promise<void>;

type Options = {
  /** How often to refresh while the tab is visible (ms). */
  intervalMs?: number;
  /** When true, skip polls (e.g. booking modal open). */
  isPaused?: () => boolean;
  /** Mark that a refresh is needed after pause ends. */
  onPausedTick?: () => void;
};

/**
 * One initial load + a single interval while the tab is visible.
 * Skips overlapping in-flight polls and background-tab churn.
 */
export function useLiveAvailabilityPoll(load: LoadFn, opts: Options = {}) {
  const { intervalMs = 15000, isPaused = () => false, onPausedTick } = opts;
  const inFlightRef = useRef(false);
  const loadRef = useRef(load);
  const pausedRef = useRef(isPaused);
  const onPausedRef = useRef(onPausedTick);

  loadRef.current = load;
  pausedRef.current = isPaused;
  onPausedRef.current = onPausedTick;

  useEffect(() => {
    let cancelled = false;
    let intervalId = 0;

    const run = async (silent: boolean, ignoreVisibility = false) => {
      if (cancelled || inFlightRef.current) return;
      if (
        !ignoreVisibility &&
        typeof document !== "undefined" &&
        document.visibilityState === "hidden"
      ) {
        return;
      }
      if (pausedRef.current()) {
        onPausedRef.current?.();
        return;
      }
      inFlightRef.current = true;
      try {
        await loadRef.current({ silent });
      } finally {
        inFlightRef.current = false;
      }
    };

    // The FIRST load must happen regardless of tab visibility — a backgrounded
    // or webview tab was leaving the grid stuck on the skeleton forever.
    void run(false, true);

    intervalId = window.setInterval(() => {
      void run(true);
    }, intervalMs);

    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        void run(true);
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [intervalMs]);
}
