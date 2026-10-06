"use client";

import Link from "next/link";
import { useId } from "react";
import { PRACTICE_CLASSES } from "@/lib/content/classSchedules";

type Props = {
  open: boolean;
  onClose: () => void;
};

const sora = { fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" } as const;

function ClassIcon({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <rect x="3" y="3" width="18" height="13" rx="2" fill="var(--red)" opacity="0.9" />
      <path d="M6 7h8M6 10h5" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
      <path d="M8 16h8v2H8v-2z" fill="var(--ink)" opacity="0.2" />
      <circle cx="8" cy="20" r="1.6" fill="var(--green)" />
      <circle cx="12" cy="20" r="1.6" fill="var(--red)" />
      <circle cx="16" cy="20" r="1.6" fill="var(--green)" />
    </svg>
  );
}

/** Calendar-check icon for the attendance CTA */
function AttendanceIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden className="shrink-0">
      <rect x="3" y="5" width="18" height="16" rx="2.5" stroke="currentColor" strokeWidth="2" />
      <path d="M3 10h18M8 3v4M16 3v4" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      <path d="M9 15.5l2 2 4-4.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** Video / Meet join icon for Join Class CTA */
function JoinClassIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden className="shrink-0">
      <rect x="2" y="5" width="14" height="14" rx="2.5" stroke="#fff" strokeWidth="2" />
      <path
        d="M16 10.2 21 7v10l-5-3.2"
        stroke="#fff"
        strokeWidth="2"
        strokeLinejoin="round"
        fill="none"
      />
      <circle cx="9" cy="12" r="2.2" fill="#fff" />
    </svg>
  );
}

export function PracticeClassesModal({ open, onClose }: Props) {
  const titleId = useId();

  if (!open) return null;

  return (
    <div
      className="cubic-modal-backdrop fixed inset-0 z-50 flex items-end justify-center sm:items-center sm:p-5 md:p-8"
      style={{
        background: "rgba(15, 18, 22, 0.48)",
        backdropFilter: "blur(6px)",
        WebkitBackdropFilter: "blur(6px)",
      }}
      role="presentation"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="cubic-modal-panel cubic-modal-sheet relative flex w-full max-w-[820px] flex-col overflow-hidden rounded-t-[18px] bg-white shadow-[0_24px_64px_rgba(15,18,22,0.28)] sm:rounded-[14px]"
        style={{ ...sora, maxHeight: "min(92dvh, 100%)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="relative flex shrink-0 flex-col items-center justify-center px-10 pb-3.5 pt-2.5 sm:px-14 sm:py-5"
          style={{
            background: "linear-gradient(135deg, #d32f2f 0%, #b71c1c 55%, #9a1515 100%)",
          }}
        >
          <div className="mb-2 h-1 w-10 rounded-full bg-white/35 sm:hidden" />
          <h3
            id={titleId}
            className="px-6 text-center text-[16px] font-black tracking-[-0.02em] text-white sm:text-[20px]"
            style={sora}
          >
            Otter &amp; Pronunciation
          </h3>
          <p
            className="mt-1 max-w-[34rem] px-2 text-center text-[12px] font-medium leading-snug text-white/85 sm:text-[13px]"
            style={sora}
          >
            Step in live — build clarity, confidence, and interview-ready speaking skills
          </p>

          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="absolute right-2 top-2 flex h-11 w-11 items-center justify-center rounded-[8px] text-white transition hover:bg-white/16 sm:right-3 sm:top-1/2 sm:h-9 sm:w-9 sm:-translate-y-1/2"
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

        <div
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-[#f7f8fa] px-4 py-5 sm:px-8 sm:py-7"
          style={{ WebkitOverflowScrolling: "touch" }}
        >
          <div className="mx-auto flex w-full max-w-[720px] flex-col gap-4 sm:gap-6">
            {PRACTICE_CLASSES.map((group) => (
              <section
                key={group.id}
                className="flex flex-col rounded-[14px] border border-[var(--line)] bg-white p-4 shadow-[0_2px_10px_rgba(15,18,22,0.05)] sm:p-6"
              >
                <div className="mb-3.5 flex items-start gap-3 sm:mb-4">
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[11px] bg-[#fdecea] sm:h-11 sm:w-11">
                    <ClassIcon size={20} />
                  </div>
                  <div className="min-w-0 pt-0.5">
                    <h4
                      className="text-[15px] font-bold tracking-[-0.02em] text-[var(--ink)] sm:text-[17px]"
                      style={sora}
                    >
                      {group.title}
                    </h4>
                    <p
                      className="mt-1 text-[12px] leading-snug text-[#6f6f6f] sm:text-[12.5px]"
                      style={sora}
                    >
                      {group.tagline}
                    </p>
                  </div>
                </div>

                <div className="mb-4 flex flex-1 flex-col gap-0 rounded-[12px] bg-[#f7f8fa] px-3.5 py-3 sm:mb-5 sm:px-4 sm:py-3.5">
                  {group.sessions.map((session, idx) => (
                    <div
                      key={`${session.label}-${session.time}-${idx}`}
                      className="flex flex-col gap-0.5 border-b border-[#e8ebef] py-2.5 first:pt-0 last:border-0 last:pb-0 sm:flex-row sm:items-baseline sm:justify-between sm:gap-3"
                    >
                      <span
                        className="shrink-0 text-[12px] font-bold text-[var(--ink)] sm:text-[12.5px]"
                        style={sora}
                      >
                        {session.label}
                      </span>
                      <span
                        className="text-[13px] font-semibold text-[var(--ink)] sm:text-right sm:text-[13.5px]"
                        style={sora}
                      >
                        {session.time}
                      </span>
                    </div>
                  ))}
                </div>

                <div className="mt-auto grid gap-2.5 sm:grid-cols-2">
                  <a
                    href={group.joinUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="cubic-download-btn"
                    style={{
                      ...sora,
                      width: "100%",
                      justifyContent: "center",
                      minHeight: 48,
                      fontSize: 13.5,
                      gap: 10,
                    }}
                  >
                    <JoinClassIcon size={17} />
                    <span>Join Class</span>
                  </a>
                  <Link
                    href="/attendance"
                    onClick={onClose}
                    className="inline-flex items-center justify-center gap-2.5 rounded-[10px] border border-[#d4dae0] bg-white text-[13.5px] font-semibold text-[var(--ink)] transition hover:bg-[#f5f6f8]"
                    style={{ ...sora, minHeight: 48 }}
                  >
                    <AttendanceIcon size={17} />
                    <span>View attendance</span>
                  </Link>
                </div>
              </section>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
