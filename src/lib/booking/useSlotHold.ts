"use client";

import { useEffect, useRef, useState } from "react";

export type SlotHoldKind = "interview" | "phone";

export type ReserveResult = {
  ok: boolean;
  holdId?: string;
  expiresAt?: string;
  error?: string;
};

/**
 * Reserve a slot BEFORE the booking modal is allowed to open. The availability
 * grid calls this the moment a candidate clicks "Book" — if the last seat was
 * just taken by someone else the caller gets `{ ok: false }` and shows a toast
 * instead of opening the form.
 */
export async function reserveSlot(
  kind: SlotHoldKind,
  dateIso: string,
  startMin: number,
  durationMin: number
): Promise<ReserveResult> {
  try {
    const res = await fetch("/api/slot-hold", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind, dateIso, startMin, durationMin }),
    });
    const json = (await res.json().catch(() => ({}))) as {
      ok?: boolean;
      holdId?: string;
      expiresAt?: string;
      error?: string;
    };
    if (!res.ok || !json.ok || !json.holdId) {
      return {
        ok: false,
        error:
          json.error || "This time is already being booked. Choose another slot.",
      };
    }
    return { ok: true, holdId: json.holdId, expiresAt: json.expiresAt };
  } catch {
    return { ok: false, error: "Could not reserve this slot. Try again." };
  }
}

type Args = {
  open: boolean;
  kind: SlotHoldKind;
  dateIso: string;
  startMin: number | null;
  durationMin: number;
  emergency: boolean;
  /**
   * Hold already acquired by the availability grid via `reserveSlot`. When set,
   * the hook adopts it — no second acquire — and only keeps it alive with a
   * heartbeat, releasing it when the modal closes.
   */
  providedHoldId?: string;
};

type HoldJson = {
  ok?: boolean;
  holdId?: string;
  error?: string;
};

function releaseHold(id: string) {
  void fetch("/api/slot-hold/release", {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ holdId: id }),
    keepalive: true,
  });
}

export function useSlotHold({
  open,
  kind,
  dateIso,
  startMin,
  durationMin,
  emergency,
  providedHoldId,
}: Args) {
  const [holdId, setHoldId] = useState("");
  const [holdError, setHoldError] = useState("");
  const [ready, setReady] = useState(false);
  const [reserving, setReserving] = useState(false);
  const holdIdRef = useRef("");
  const initialStartMinRef = useRef<number | null>(null);
  const initialHoldIdRef = useRef<string | undefined>(undefined);
  const shownRef = useRef(false);

  useEffect(() => {
    holdIdRef.current = holdId;
  }, [holdId]);

  // Track the initial slot for which providedHoldId was granted.
  useEffect(() => {
    if (open) {
      if (initialStartMinRef.current === null) {
        initialStartMinRef.current = startMin;
        initialHoldIdRef.current = providedHoldId;
      }
    } else {
      initialStartMinRef.current = null;
      initialHoldIdRef.current = undefined;
      shownRef.current = false;
      setHoldId("");
      holdIdRef.current = "";
      setHoldError("");
      setReady(false);
      setReserving(false);
    }
  }, [open, startMin, providedHoldId]);

  useEffect(() => {
    if (!open) return;

    if (emergency) {
      setHoldId("");
      holdIdRef.current = "";
      setHoldError("");
      setReady(true);
      setReserving(false);
      shownRef.current = true;
      return;
    }

    // Case 1: Active startMin matches initial grid reservation -> adopt providedHoldId.
    const isInitialSlot =
      initialHoldIdRef.current &&
      initialStartMinRef.current !== null &&
      startMin === initialStartMinRef.current;

    if (isInitialSlot && initialHoldIdRef.current) {
      const idToAdopt = initialHoldIdRef.current;
      setHoldError("");
      setHoldId(idToAdopt);
      holdIdRef.current = idToAdopt;
      setReady(true);
      setReserving(false);
      shownRef.current = true;

      const heartbeatId = window.setInterval(() => {
        const id = holdIdRef.current;
        if (!id) return;
        void fetch("/api/slot-hold/heartbeat", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ holdId: id }),
        }).then(async (r) => {
          if (r.ok) return;
          const body = (await r.json().catch(() => ({}))) as HoldJson;
          setHoldError(
            body.error || "Your hold on this slot expired. Close and pick it again."
          );
          holdIdRef.current = "";
          setHoldId("");
        });
      }, 60_000);

      return () => {
        window.clearInterval(heartbeatId);
        const id = holdIdRef.current;
        if (id) {
          holdIdRef.current = "";
          releaseHold(id);
        }
      };
    }

    // Case 2: User changed the time (or opened emergency/unheld slot) -> release old hold and acquire new one.
    if (startMin == null) {
      // Time cleared or empty: release existing hold
      const prevId = holdIdRef.current;
      if (prevId) {
        holdIdRef.current = "";
        setHoldId("");
        releaseHold(prevId);
      }
      setHoldError("");
      setReserving(false);
      if (!shownRef.current) {
        setReady(true);
        shownRef.current = true;
      }
      return;
    }

    let cancelled = false;
    let heartbeatId = 0;

    // Release previously held slot when time changes.
    const prevHold = holdIdRef.current;
    if (prevHold) {
      holdIdRef.current = "";
      setHoldId("");
      releaseHold(prevHold);
    }
    setHoldError("");
    setReserving(true);

    async function acquire() {
      const res = await fetch("/api/slot-hold", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind, dateIso, startMin, durationMin }),
      });
      const json = (await res.json().catch(() => ({}))) as HoldJson;
      if (cancelled) {
        if (json.holdId) releaseHold(json.holdId);
        return;
      }
      setReserving(false);
      if (!res.ok || !json.ok || !json.holdId) {
        setHoldError(
          json.error || "This time is already being booked. Choose another slot."
        );
        shownRef.current = true;
        setReady(true);
        return;
      }
      holdIdRef.current = json.holdId;
      setHoldId(json.holdId);
      shownRef.current = true;
      setReady(true);

      heartbeatId = window.setInterval(() => {
        const id = holdIdRef.current;
        if (!id) return;
        void fetch("/api/slot-hold/heartbeat", {
          method: "POST",
          credentials: "include",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ holdId: id }),
        }).then(async (r) => {
          if (r.ok) return;
          const body = (await r.json().catch(() => ({}))) as HoldJson;
          setHoldError(body.error || "Your hold on this slot expired. Close and pick it again.");
          holdIdRef.current = "";
          setHoldId("");
        });
      }, 60_000);
    }

    const timer = window.setTimeout(() => {
      void acquire().catch(() => {
        if (!cancelled) {
          setReserving(false);
          setHoldError("Could not reserve this slot. Close and try again.");
          shownRef.current = true;
          setReady(true);
        }
      });
    }, 280);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      if (heartbeatId) window.clearInterval(heartbeatId);
      const id = holdIdRef.current;
      if (id) {
        holdIdRef.current = "";
        releaseHold(id);
      }
    };
  }, [open, kind, dateIso, startMin, durationMin, emergency]);

  return { holdId, holdError, ready, reserving };
}
