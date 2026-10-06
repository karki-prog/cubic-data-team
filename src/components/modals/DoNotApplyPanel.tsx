"use client";

import { Skel } from "@/components/ui/Skeleton";
import { useId } from "react";
import { useDoNotApplyCompanies } from "@/lib/content/useDoNotApply";

type Props = {
  open: boolean;
  onClose: () => void;
};

/** Compact bottom-right panel for availability pages (not the full dashboard modal). */
export function DoNotApplyPanel({ open, onClose }: Props) {
  const titleId = useId();
  const { companies, loading, error } = useDoNotApplyCompanies(open);

  if (!open) return null;

  return (
    <div
      className={`pca-dna-overlay${open ? " show" : ""}`}
      role="presentation"
      onClick={onClose}
    >
      <div
        className="pca-dna-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="pca-dna-head">
          <div>
            <h2 id={titleId}>Do Not Apply</h2>
            <p>Completely avoid these companies</p>
          </div>
          <button
            type="button"
            className="pca-dna-close"
            aria-label="Close"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <ul className="pca-dna-list">
          {loading
            ? Array.from({ length: 6 }).map((_, i) => (
                <li key={`skel-${i}`} aria-hidden>
                  <Skel w="55%" h={12} />
                  <Skel w="80%" h={10} style={{ marginTop: 6 }} />
                </li>
              ))
            : null}
          {error && !loading ? <li className="pca-dna-reason">{error}</li> : null}
          {companies.map((company, index) => (
            <li key={`${company.name}\0${company.reason}\0${index}`}>
              <div className="pca-dna-name">{company.name}</div>
              {company.reason ? (
                <div className="pca-dna-reason">{company.reason}</div>
              ) : null}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
