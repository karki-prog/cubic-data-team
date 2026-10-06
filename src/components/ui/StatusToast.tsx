"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export const TOAST_MS = 4200;
const EXIT_MS = 280;

export type StatusToastKind = "success" | "error";

type ToastState = {
  id: number;
  kind: StatusToastKind;
  title?: string;
  message: string;
};

export function useStatusToast() {
  const [toast, setToast] = useState<ToastState | null>(null);

  const show = useCallback((kind: StatusToastKind, message: string, title?: string) => {
    const text = message.trim();
    if (!text) return;
    setToast({ id: Date.now(), kind, message: text, title: title?.trim() || undefined });
  }, []);

  const dismiss = useCallback(() => setToast(null), []);

  return { toast, show, dismiss };
}

function SuccessIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M20 6 9 17l-5-5"
        stroke="currentColor"
        strokeWidth="2.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ErrorIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M18 6 6 18M6 6l12 12"
        stroke="currentColor"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function StatusToast({
  toast,
  durationMs = TOAST_MS,
  onDismiss,
}: {
  toast: ToastState | null;
  durationMs?: number;
  onDismiss: () => void;
}) {
  const [mounted, setMounted] = useState(false);
  const [shown, setShown] = useState<ToastState | null>(null);
  const [leaving, setLeaving] = useState(false);
  const visibleRef = useRef(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  useEffect(() => {
    if (toast) {
      visibleRef.current = true;
      setShown(toast);
      setLeaving(false);
      const hide = window.setTimeout(onDismiss, durationMs);
      return () => window.clearTimeout(hide);
    }
    if (!visibleRef.current) return;
    setLeaving(true);
    const exit = window.setTimeout(() => {
      visibleRef.current = false;
      setShown(null);
      setLeaving(false);
    }, EXIT_MS);
    return () => window.clearTimeout(exit);
  }, [toast, durationMs, onDismiss]);

  if (!mounted || !shown) return null;

  const isError = shown.kind === "error";
  const title = shown.title;

  return createPortal(
    <div
      key={shown.id}
      className={`cubic-status-toast ${isError ? "cubic-status-toast-error" : "cubic-status-toast-success"}${leaving ? " is-leaving" : ""}`}
      role={isError ? "alert" : "status"}
    >
      <div className="cubic-status-toast-row">
        <span className="cubic-status-toast-icon" aria-hidden>
          {isError ? <ErrorIcon /> : <SuccessIcon />}
        </span>
        <div className="cubic-status-toast-copy">
          {title ? <p className="cubic-status-toast-title">{title}</p> : null}
          <p className="cubic-status-toast-msg">{shown.message}</p>
        </div>
      </div>
      <div
        className="cubic-status-toast-progress"
        style={{ animationDuration: `${durationMs}ms` }}
      />
    </div>,
    document.body
  );
}
