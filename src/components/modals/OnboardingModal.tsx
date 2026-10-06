"use client";

import { useId } from "react";
import { ArrowIcon } from "@/components/ui/ArrowIcon";
import { ONBOARDING_DOCS, type OnboardingDoc } from "@/lib/content/onboardingDocs";

type Props = {
  open: boolean;
  onClose: () => void;
  onDownload?: (doc: OnboardingDoc) => void;
};

function FileTypeIcon({ type }: { type: string }) {
  const kind = type.toUpperCase();
  const isExcel = kind === "XLSX" || kind === "XLS" || kind === "CSV";
  const isPdf = kind === "PDF";
  const fill = isExcel ? "#0F7B3A" : isPdf ? "#C5221F" : "#2B579A";
  const label = isExcel ? "XLS" : isPdf ? "PDF" : "DOC";

  return (
    <svg
      viewBox="0 0 40 48"
      fill="none"
      aria-hidden
      className="h-10 w-[33px] shrink-0 sm:h-12 sm:w-10"
    >
      <path
        d="M8 2h16l12 12v30a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4V6a4 4 0 0 1 4-4z"
        fill="#F4F6F8"
        stroke="#D8DEE6"
        strokeWidth="1.2"
      />
      <path d="M24 2v10a2 2 0 0 0 2 2h10" fill="#E8EEF5" stroke="#D8DEE6" strokeWidth="1.2" />
      <rect x="4" y="28" width="32" height="14" rx="2" fill={fill} />
      <text
        x="20"
        y="38"
        textAnchor="middle"
        fill="#fff"
        fontSize="9"
        fontWeight="800"
        fontFamily="var(--font-sora), Helvetica Neue, Arial, sans-serif"
        letterSpacing="0.4"
      >
        {label}
      </text>
      <path d="M10 12h12M10 17h14M10 22h10" stroke="#C5CDD8" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden className="shrink-0">
      <path
        d="M8 2.5v7.2M5.2 7.2 8 10l2.8-2.8"
        stroke="#ffffff"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="M3 12.5h10" stroke="#ffffff" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

export function OnboardingModal({ open, onClose, onDownload }: Props) {
  const titleId = useId();

  if (!open) return null;

  const docs = ONBOARDING_DOCS;
  const hasDocs = docs.length > 0;

  const handleDownload = (doc: OnboardingDoc) => {
    onDownload?.(doc);
  };

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
        className="cubic-modal-panel cubic-modal-sheet relative flex w-full max-w-[920px] flex-col overflow-hidden rounded-t-[18px] bg-white shadow-[0_24px_64px_rgba(15,18,22,0.28)] sm:h-[min(88vh,820px)] sm:rounded-[14px]"
        style={{ height: "min(94dvh, 940px)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="relative flex shrink-0 flex-col items-center justify-center px-11 pb-3 pt-2 sm:px-12 sm:py-4"
          style={{
            background: "linear-gradient(135deg, #d32f2f 0%, #b71c1c 55%, #9a1515 100%)",
          }}
        >
          <div className="mb-2 h-1 w-10 rounded-full bg-white/35 sm:hidden" />
          <h3
            id={titleId}
            className="truncate text-center text-[15px] font-black tracking-[-0.02em] text-white sm:text-[18px]"
            style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
          >
            Onboarding List
          </h3>

          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="absolute right-2 top-2 flex h-10 w-10 items-center justify-center rounded-[8px] text-white transition hover:bg-white/16 sm:right-3 sm:top-1/2 sm:h-9 sm:w-9 sm:-translate-y-1/2"
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

        <div className="flex min-h-0 flex-1 flex-col bg-[#f7f8fa]">
          <div className="flex items-center justify-between gap-3 border-b border-[var(--line)] bg-white px-4 py-2.5 sm:px-6">
            <p className="text-[12px] font-semibold text-[#6f6f6f]">
              {hasDocs ? (
                <>
                  <span className="font-bold text-[var(--ink)]">{docs.length}</span> document
                  {docs.length === 1 ? "" : "s"}
                </>
              ) : (
                "Documents"
              )}
            </p>
            <p className="text-[11px] font-medium text-[#9aa0a6]">Scroll to browse</p>
          </div>

          <div
            className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3 sm:px-5 sm:py-4"
            style={{ WebkitOverflowScrolling: "touch" }}
          >
            {hasDocs ? (
              <ul className="grid grid-cols-1 gap-2.5 pb-4 md:grid-cols-2 md:pb-1">
                {docs.map((doc) => (
                  <li key={doc.id}>
                    <div className="flex flex-row items-center gap-3 rounded-[12px] border border-[var(--line)] bg-white px-3 py-3 shadow-[0_1px_2px_rgba(0,0,0,0.04)] sm:items-start sm:gap-3.5 sm:px-4 sm:py-3.5">
                      <div className="flex min-w-0 flex-1 items-start gap-3">
                        <FileTypeIcon type={doc.type} />

                        <div className="min-w-0 flex-1">
                          <p
                            className="whitespace-normal break-words text-[13px] font-semibold leading-snug tracking-[-0.01em] text-[var(--ink)] sm:text-[13.5px]"
                            style={{
                              fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif",
                            }}
                          >
                            {doc.title}
                          </p>
                          <p className="mt-1 whitespace-normal break-words text-[11px] font-semibold uppercase tracking-[0.04em] text-[#8b9198] sm:text-[11.5px]">
                            {doc.type}
                            {doc.description ? ` · ${doc.description}` : ""}
                          </p>
                        </div>
                      </div>

                      {doc.openInNewTab ? (
                        <a
                          href={doc.href}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={() => handleDownload(doc)}
                          aria-label={`Open ${doc.title}`}
                          className="inline-flex h-10 w-10 shrink-0 items-center justify-center gap-1.5 rounded-[9px] border border-[var(--line)] bg-white px-0 text-[12px] font-bold text-[var(--ink)] shadow-[0_1px_2px_rgba(0,0,0,0.04)] sm:w-auto sm:gap-2 sm:px-3.5 sm:text-[12.5px]"
                        >
                          <span className="hidden sm:inline">Open</span>
                          <ArrowIcon color="currentColor" size={13} />
                        </a>
                      ) : (
                        <a
                          href={doc.href}
                          download
                          onClick={() => handleDownload(doc)}
                          aria-label={`Download ${doc.title}`}
                          className="cubic-download-btn shrink-0 !justify-center max-sm:!h-10 max-sm:!w-10 max-sm:!px-0"
                        >
                          <DownloadIcon />
                          <span className="hidden sm:inline">Download</span>
                        </a>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            ) : (
              <div className="rounded-[12px] border border-dashed border-[#e4b4b1] bg-white px-5 py-12 text-center">
                <p className="text-[14px] font-bold text-[var(--ink)]">No documents yet</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
