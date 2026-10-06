"use client";

import { Skel } from "@/components/ui/Skeleton";
import { useEffect, useId, useMemo } from "react";
import { useDoNotApplyCompanies } from "@/lib/content/useDoNotApply";

type Props = {
  open: boolean;
  onClose: () => void;
};

const sora = { fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" } as const;

export function DoNotApplyModal({ open, onClose }: Props) {
  const titleId = useId();
  const { companies, loading, error } = useDoNotApplyCompanies(open);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = previousOverflow;
    };
  }, [open, onClose]);

  const rows = useMemo(
    () =>
      [...companies].sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
      ),
    [companies]
  );

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center sm:items-center sm:p-6"
      style={{ background: "rgba(15, 18, 22, 0.45)" }}
      role="presentation"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="cubic-dna-dialog flex w-full max-w-[840px] flex-col overflow-hidden rounded-t-[16px] bg-white sm:rounded-[12px]"
        style={sora}
        onClick={(event) => event.stopPropagation()}
      >
        <header
          className="relative flex shrink-0 items-start justify-between gap-4 px-5 py-4 sm:px-6"
          style={{
            background: "linear-gradient(135deg, #d32f2f 0%, #b71c1c 55%, #9a1515 100%)",
          }}
        >
          <div className="min-w-0 pr-10">
            <h2
              id={titleId}
              className="text-[17px] font-semibold tracking-[-0.02em] text-white sm:text-[18px]"
            >
              Do Not Apply List
            </h2>
            <p className="mt-1 text-[13px] leading-snug text-white/85">
              Companies to skip before applying
              {loading
                ? ""
                : ` · ${rows.length} ${rows.length === 1 ? "company" : "companies"}`}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="absolute right-3 top-3 flex h-9 w-9 items-center justify-center rounded-[8px] text-white hover:bg-white/16"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden>
              <path
                d="M3 3l8 8M11 3L3 11"
                stroke="currentColor"
                strokeWidth="1.7"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-white">
          {loading ? (
            <div aria-busy="true" aria-label="Loading companies">
              {Array.from({ length: 9 }).map((_, i) => (
                <div key={i} className="flex items-center gap-6 border-b border-[var(--line)] px-5 py-3.5 sm:px-6">
                  <Skel w="32%" h={12} />
                  <Skel w="48%" h={12} />
                </div>
              ))}
            </div>
          ) : null}
          {error && !loading ? (
            <p className="px-6 py-10 text-[13.5px] text-[#b71c1c]">{error}</p>
          ) : null}
          {!loading && !error && rows.length === 0 ? (
            <p className="px-6 py-10 text-[13.5px] text-[#5f6368]">No companies on this list.</p>
          ) : null}
          {!loading && !error && rows.length > 0 ? (
            <table className="w-full border-collapse text-left">
              <thead className="sticky top-0 hidden bg-[#f7f8fa] sm:table-header-group">
                <tr className="border-b border-[var(--line)]">
                  <th className="w-[38%] px-5 py-2.5 text-[11px] font-semibold uppercase tracking-[0.06em] text-[#5f6368] sm:px-6">
                    Company
                  </th>
                  <th className="px-5 py-2.5 text-[11px] font-semibold uppercase tracking-[0.06em] text-[#5f6368] sm:px-6">
                    Reason
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((company, index) => (
                  <tr
                    key={`${company.name}\0${company.reason}\0${index}`}
                    className="block border-b border-[#eef0f2] py-3 align-top last:border-b-0 sm:table-row sm:py-0"
                  >
                    <td className="block px-5 text-[14px] font-semibold sm:table-cell sm:py-3 sm:text-[13.5px] leading-snug break-words text-[var(--ink)] sm:px-6">
                      {company.name}
                    </td>
                    <td className={`${company.reason ? "block" : "hidden"} px-5 pt-1 text-[13px] leading-relaxed sm:table-cell sm:py-3 break-words text-[#3c4043] sm:px-6`}>
                      {company.reason || "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : null}
        </div>
      </div>
    </div>
  );
}
