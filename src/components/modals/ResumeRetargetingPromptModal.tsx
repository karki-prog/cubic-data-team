"use client";

import { Fragment, useCallback, useId, useMemo, useState, type ReactNode } from "react";
import { COVER_LETTER_PROMPT } from "@/lib/content/coverLetterPrompt";

type Props = {
  open: boolean;
  onClose: () => void;
};

const sora = { fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" } as const;

function CopyIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden className="shrink-0">
      <rect x="5" y="5" width="8" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M4 11H3.5A1.5 1.5 0 0 1 2 9.5v-7A1.5 1.5 0 0 1 3.5 1h7A1.5 1.5 0 0 1 12 2.5V3"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function renderInline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*)/g).map((part, index) => {
    if (part.startsWith("**") && part.endsWith("**")) {
      return (
        <strong key={index} className="font-semibold text-black">
          {part.slice(2, -2)}
        </strong>
      );
    }
    return <Fragment key={index}>{part}</Fragment>;
  });
}

/** Render prompt as readable markdown — never rewrite or invent text. */
function PromptDocument({ source }: { source: string }) {
  const blocks = useMemo(() => {
    const lines = source.replace(/\r\n/g, "\n").split("\n");
    const nodes: ReactNode[] = [];
    let listItems: string[] = [];
    let key = 0;
    /** After "Recommended structure:" keep lines as plain text (no fake headings). */
    let plainTail = false;

    const flushList = () => {
      if (!listItems.length) return;
      nodes.push(
        <ul key={`ul-${key++}`} className="my-3 space-y-2 pl-0">
          {listItems.map((item) => (
            <li
              key={`li-${key++}`}
              className="relative pl-5 text-[13.5px] leading-[1.65] text-black before:absolute before:left-0 before:top-[0.62em] before:h-1.5 before:w-1.5 before:rounded-full before:bg-black before:content-['']"
              style={sora}
            >
              {renderInline(item)}
            </li>
          ))}
        </ul>,
      );
      listItems = [];
    };

    for (const raw of lines) {
      const line = raw.trimEnd();
      const trimmed = line.trim();

      if (!trimmed) {
        flushList();
        continue;
      }

      if (trimmed === "---") {
        flushList();
        plainTail = false;
        nodes.push(<hr key={`hr-${key++}`} className="my-7 border-0 border-t border-[#e5e7eb]" />);
        continue;
      }

      if (/^recommended structure/i.test(trimmed)) {
        flushList();
        plainTail = true;
        nodes.push(
          <p
            key={`p-${key++}`}
            className="my-2.5 text-[13.5px] leading-[1.7] text-black sm:text-[14px]"
            style={sora}
          >
            {renderInline(trimmed)}
          </p>,
        );
        continue;
      }

      // Resume template example only — keep as plain lines, not fake headings.
      if (plainTail) {
        flushList();
        const display = trimmed.replace(/^#{1,3}\s+/, "");
        nodes.push(
          <p
            key={`p-${key++}`}
            className="my-2 text-[13.5px] leading-[1.7] text-black sm:text-[14px]"
            style={sora}
          >
            {renderInline(display.startsWith("* ") ? display : display)}
          </p>,
        );
        continue;
      }

      if (trimmed.startsWith("# ")) {
        flushList();
        nodes.push(
          <h2
            key={`h1-${key++}`}
            className="mb-4 mt-1 text-[19px] font-bold tracking-[-0.03em] text-black sm:text-[21px]"
            style={sora}
          >
            {trimmed.slice(2)}
          </h2>,
        );
        continue;
      }

      if (trimmed.startsWith("## ")) {
        flushList();
        nodes.push(
          <h3
            key={`h2-${key++}`}
            className="mb-3 mt-8 text-[15px] font-bold tracking-[-0.02em] text-black first:mt-0 sm:text-[16px]"
            style={sora}
          >
            {trimmed.slice(3)}
          </h3>,
        );
        continue;
      }

      if (trimmed.startsWith("### ")) {
        flushList();
        nodes.push(
          <h4
            key={`h3-${key++}`}
            className="mb-2.5 mt-6 text-[14px] font-semibold tracking-[-0.02em] text-black sm:text-[15px]"
            style={sora}
          >
            {trimmed.slice(4)}
          </h4>,
        );
        continue;
      }

      if (trimmed.startsWith("* ")) {
        listItems.push(trimmed.slice(2));
        continue;
      }

      flushList();
      nodes.push(
        <p
          key={`p-${key++}`}
          className="my-2.5 text-[13.5px] leading-[1.7] text-black sm:text-[14px]"
          style={sora}
        >
          {renderInline(trimmed)}
        </p>,
      );
    }

    flushList();
    return nodes;
  }, [source]);

  return <article className="prompt-document">{blocks}</article>;
}

export function ResumeRetargetingPromptModal({ open, onClose }: Props) {
  const titleId = useId();
  const [copied, setCopied] = useState(false);

  const copyPrompt = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(COVER_LETTER_PROMPT);
    } catch {
      const area = document.createElement("textarea");
      area.value = COVER_LETTER_PROMPT;
      area.style.position = "fixed";
      area.style.left = "-9999px";
      document.body.appendChild(area);
      area.select();
      document.execCommand("copy");
      document.body.removeChild(area);
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 2200);
  }, []);

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
        className="cubic-modal-panel cubic-modal-sheet relative flex w-full max-w-[min(1140px,98vw)] flex-col overflow-hidden rounded-t-[18px] bg-white shadow-[0_24px_64px_rgba(15,18,22,0.28)] sm:rounded-[14px]"
        style={{ ...sora, maxHeight: "min(96dvh, 100%)", height: "min(94dvh, 960px)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="relative flex shrink-0 items-center gap-3 px-4 pb-3.5 pt-2.5 sm:px-6 sm:py-4"
          style={{
            background: "linear-gradient(135deg, #d32f2f 0%, #b71c1c 55%, #9a1515 100%)",
          }}
        >
          <div className="mb-2 h-1 w-10 rounded-full bg-white/35 sm:hidden absolute left-1/2 top-1.5 -translate-x-1/2" />

          <div className="min-w-0 flex-1 pt-1 sm:pt-0 sm:pr-28">
            <h3
              id={titleId}
              className="truncate text-[16px] font-black tracking-[-0.02em] text-white sm:text-[20px]"
              style={sora}
            >
              Cover Letter Tailoring Prompt
            </h3>
          </div>

          <button
            type="button"
            onClick={copyPrompt}
            className="absolute right-12 top-2 inline-flex h-10 items-center gap-2 rounded-[10px] border border-white/30 bg-white/12 px-3.5 text-[12.5px] font-semibold text-white transition hover:bg-white/20 sm:right-14 sm:top-1/2 sm:h-10 sm:-translate-y-1/2 sm:px-4 sm:text-[13px]"
            style={sora}
          >
            <CopyIcon />
            <span>{copied ? "Copied" : "Copy prompt"}</span>
          </button>

          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="absolute right-2 top-2 flex h-10 w-10 items-center justify-center rounded-[8px] text-white transition hover:bg-white/16 sm:top-1/2 sm:-translate-y-1/2"
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
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-white px-5 py-6 sm:px-10 sm:py-9"
          style={{ WebkitOverflowScrolling: "touch" }}
        >
          <PromptDocument source={COVER_LETTER_PROMPT} />
        </div>
      </div>
    </div>
  );
}
