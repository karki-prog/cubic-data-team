"use client";

import { useCallback, useEffect, useId, useRef, useState } from "react";
import Link from "next/link";
import { ArrowIcon } from "@/components/ui/ArrowIcon";
import { StatusToast, useStatusToast } from "@/components/ui/StatusToast";
import { DoNotApplyModal } from "@/components/modals/DoNotApplyModal";
import { OnboardingModal } from "@/components/modals/OnboardingModal";
import { PracticeClassesModal } from "@/components/modals/PracticeClassesModal";
import { ResumeRetargetingPromptModal } from "@/components/modals/ResumeRetargetingPromptModal";
import { ToolGraphic } from "./ToolGraphic";
import { TEAM_TOOLS, type TeamTool } from "@/lib/content/tools";
import { CUBIC_TOOL_EVENT } from "@/lib/dashboard/events";

const SIGN_IN_TOAST_MS = 2200;

type ModalKind =
  | "coming-soon"
  | "onboarding"
  | "practice-classes"
  | "do-not-apply"
  | "resume-prompt"
  | null;

export function DashboardGrid() {
  const [modalKind, setModalKind] = useState<ModalKind>(null);
  const [comingSoonTool, setComingSoonTool] = useState<TeamTool | null>(null);
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [isPhone, setIsPhone] = useState(false);
  const status = useStatusToast();
  const titleId = useId();
  const descId = useId();
  const redirectTimer = useRef<number | null>(null);

  useEffect(() => {
    const mq = window.matchMedia("(max-width: 639px)");
    const sync = () => setIsPhone(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    fetch("/api/auth/me", { credentials: "same-origin" })
      .then((r) => r.json())
      .then((data: { ok?: boolean; user?: unknown }) => {
        setSignedIn(Boolean(data.ok && data.user));
      })
      .catch(() => setSignedIn(false));
    return () => {
      if (redirectTimer.current) window.clearTimeout(redirectTimer.current);
    };
  }, []);

  const promptSignIn = useCallback((nextPath: string) => {
    status.show(
      "error",
      "Taking you to sign in — use the email you gave the Cubic team.",
      "Sign in required"
    );
    const next = nextPath.startsWith("/") && !nextPath.startsWith("//") ? nextPath : "/";
    if (redirectTimer.current) window.clearTimeout(redirectTimer.current);
    redirectTimer.current = window.setTimeout(() => {
      window.location.href = `/login?next=${encodeURIComponent(next)}`;
    }, SIGN_IN_TOAST_MS);
  }, [status.show]);

  const requireSignIn = useCallback(
    (nextPath: string) => {
      if (signedIn === true) return false;
      if (signedIn === null) return true;
      promptSignIn(nextPath);
      return true;
    },
    [promptSignIn, signedIn]
  );

  const openComingSoon = useCallback((tool: TeamTool) => {
    setComingSoonTool(tool);
    setModalKind("coming-soon");
  }, []);

  const openOnboarding = useCallback(() => {
    setModalKind("onboarding");
  }, []);

  const openPracticeClasses = useCallback(() => {
    setModalKind("practice-classes");
  }, []);

  const openDoNotApply = useCallback(() => {
    setModalKind("do-not-apply");
  }, []);

  const openResumePrompt = useCallback(() => {
    setModalKind("resume-prompt");
  }, []);

  const activateTool = useCallback(
    (tool: TeamTool) => {
      if (isPhone && !tool.mobileOk) {
        status.show(
          "error",
          "This one needs a computer. On your phone you can still open the class link and the support form.",
          "Open on a computer"
        );
        return;
      }
      if (tool.url) {
        const isInternal = tool.url.startsWith("/");
        const loginNext = isInternal ? tool.url : "/";
        if (requireSignIn(loginNext)) return;
        if (isInternal) {
          window.location.href = tool.url;
          return;
        }
        window.open(tool.url, "_blank", "noopener,noreferrer");
        return;
      }
      if (requireSignIn("/")) return;
      if (tool.action === "onboarding") openOnboarding();
      else if (tool.action === "practice-classes") openPracticeClasses();
      else if (tool.action === "do-not-apply") openDoNotApply();
      else if (tool.action === "resume-prompt") openResumePrompt();
      else openComingSoon(tool);
    },
    [
      isPhone,
      status.show,
      openComingSoon,
      openDoNotApply,
      openOnboarding,
      openPracticeClasses,
      openResumePrompt,
      requireSignIn,
    ]
  );

  useEffect(() => {
    const onTool = (event: Event) => {
      const tool = (event as CustomEvent<TeamTool>).detail;
      if (tool) activateTool(tool);
    };
    window.addEventListener(CUBIC_TOOL_EVENT, onTool);
    return () => window.removeEventListener(CUBIC_TOOL_EVENT, onTool);
  }, [activateTool]);

  const closeComingSoon = useCallback(() => {
    setModalKind(null);
    setComingSoonTool(null);
  }, []);

  const closeOnboarding = useCallback(() => {
    setModalKind(null);
  }, []);

  const closePracticeClasses = useCallback(() => {
    setModalKind(null);
  }, []);

  const closeDoNotApply = useCallback(() => {
    setModalKind(null);
  }, []);

  const closeResumePrompt = useCallback(() => {
    setModalKind(null);
  }, []);

  useEffect(() => {
    if (!modalKind) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (modalKind === "coming-soon") closeComingSoon();
      else if (modalKind === "practice-classes") closePracticeClasses();
      else if (modalKind === "do-not-apply") closeDoNotApply();
      else if (modalKind === "resume-prompt") closeResumePrompt();
      else closeOnboarding();
    };
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", onKey);
    return () => {
      document.body.style.overflow = prev;
      window.removeEventListener("keydown", onKey);
    };
  }, [
    modalKind,
    closeComingSoon,
    closeOnboarding,
    closePracticeClasses,
    closeDoNotApply,
    closeResumePrompt,
  ]);

  return (
    <>
      <div className="grid grid-cols-1 gap-5 sm:grid-cols-2 xl:grid-cols-4">
        {TEAM_TOOLS.map((tool) => {
          // On phones only the class link + support form open; the rest need a
          // wide screen, so they render as a quiet "Open on a computer" tile.
          const phoneBlocked = isPhone && !tool.mobileOk;
          const isLink = Boolean(tool.url) && !phoneBlocked;
          const isOnboarding = tool.action === "onboarding" && !phoneBlocked;
          const isPractice = tool.action === "practice-classes" && !phoneBlocked;
          const isDoNotApply = tool.action === "do-not-apply" && !phoneBlocked;
          const isResumePrompt = tool.action === "resume-prompt" && !phoneBlocked;
          const enabled = isLink || isOnboarding || isPractice || isDoNotApply || isResumePrompt;
          const isPlaceholder = !enabled && (phoneBlocked || tool.title === "Coming soon");
          const showFullIcon = enabled || !isPlaceholder;
          const shell =
            "group relative flex min-h-[220px] flex-col overflow-hidden rounded-[12px] border border-[var(--line)] bg-white text-left transition duration-200 " +
            (enabled
              ? "hover:-translate-y-0.5 hover:shadow-[0_10px_28px_rgba(0,0,0,0.08)]"
              : isPlaceholder
                ? "cursor-pointer opacity-70 hover:opacity-90"
                : "cursor-pointer hover:shadow-[0_8px_22px_rgba(0,0,0,0.06)]");

          const onTileClick = () => {
            activateTool(tool);
          };

          const ctaLabel = phoneBlocked
            ? "Open on a computer"
            : tool.cta || (enabled ? "Open" : "Coming soon");
          const cta = (
            <>
              {ctaLabel}
              <ArrowIcon color={enabled ? "var(--green)" : "var(--red)"} size={13} />
            </>
          );

          const ctaColor = enabled ? "text-[var(--green)]" : "text-[var(--red)]";

          const inner = (
            <>
              <div className="relative h-[112px] overflow-hidden bg-[linear-gradient(160deg,#fff_0%,#f8f9fb_55%,#fdecea_100%)]">
                <div className="absolute inset-0 p-3 opacity-95 transition duration-200 group-hover:scale-[1.03]">
                  <ToolGraphic id={tool.id} muted={!showFullIcon} />
                </div>
                <div className="pointer-events-none absolute inset-x-0 bottom-0 h-8 bg-gradient-to-t from-white to-transparent" />
              </div>
              <div className="flex flex-1 flex-col justify-between gap-4 p-5 pt-4">
                <div>
                  <h2
                    className={
                      "text-[15px] font-semibold tracking-[-0.02em] sm:text-[16px] " +
                      (enabled || !isPlaceholder ? "text-[var(--red)]" : "text-[#6b7280]")
                    }
                    style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
                  >
                    {tool.title}
                  </h2>
                  <p
                    className={
                      "mt-2 text-[13px] leading-snug " +
                      (enabled || !isPlaceholder ? "text-[#6f6f6f]" : "text-[#9aa0a6]")
                    }
                    style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
                  >
                    {tool.blurb}
                  </p>
                </div>
                <p
                  className={`inline-flex items-center gap-1.5 text-[12.5px] font-semibold ${ctaColor}`}
                  style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
                >
                  {cta}
                </p>
              </div>
            </>
          );

          if (isLink && tool.url) {
            const isInternal = tool.url.startsWith("/");
            const loginNext = isInternal ? tool.url : "/";
            if (isInternal) {
              return (
                <Link
                  key={tool.id}
                  href={tool.url}
                  className={shell}
                  onClick={(e) => {
                    if (requireSignIn(loginNext)) e.preventDefault();
                  }}
                >
                  {inner}
                </Link>
              );
            }
            return (
              <a
                key={tool.id}
                href={tool.url}
                target="_blank"
                rel="noopener noreferrer"
                className={shell}
                onClick={(e) => {
                  if (requireSignIn(loginNext)) e.preventDefault();
                }}
              >
                {inner}
              </a>
            );
          }

          return (
            <button key={tool.id} type="button" className={shell} onClick={onTileClick}>
              {inner}
            </button>
          );
        })}
      </div>

      {modalKind === "coming-soon" && comingSoonTool ? (
        <div
          className="cubic-modal-backdrop fixed inset-0 z-50 flex items-end justify-center sm:items-center sm:p-5 md:p-8"
          style={{
            background: "rgba(15, 18, 22, 0.48)",
            backdropFilter: "blur(6px)",
            WebkitBackdropFilter: "blur(6px)",
          }}
          role="presentation"
          onClick={closeComingSoon}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            aria-describedby={descId}
            className="cubic-modal-panel cubic-modal-sheet relative w-full max-w-[520px] overflow-hidden rounded-t-[18px] bg-white shadow-[0_24px_64px_rgba(15,18,22,0.28)] sm:rounded-[14px]"
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
                Coming soon
              </h3>

              <button
                type="button"
                aria-label="Close"
                onClick={closeComingSoon}
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

            <div className="bg-[#f7f8fa] px-5 py-8 sm:px-8 sm:py-10">
              <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-[#fdecea] sm:h-14 sm:w-14">
                <svg width="26" height="26" viewBox="0 0 24 24" fill="none" aria-hidden>
                  <path
                    d="M12 7v5.5M12 16.5h.01"
                    stroke="var(--red)"
                    strokeWidth="2.2"
                    strokeLinecap="round"
                  />
                  <circle cx="12" cy="12" r="9" stroke="var(--red)" strokeWidth="1.8" />
                </svg>
              </div>

              <h4
                className="mt-4 text-center text-[17px] font-semibold tracking-[-0.02em] text-[var(--ink)] sm:mt-5 sm:text-[20px]"
                style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
              >
                {comingSoonTool.title}
              </h4>
              <p
                id={descId}
                className="mx-auto mt-3 max-w-sm text-center text-[13.5px] leading-relaxed text-[#6f6f6f] sm:text-[14px]"
                style={{ fontFamily: "var(--font-sora), Helvetica Neue, Arial, sans-serif" }}
              >
                This tool isn’t live yet. We’re finishing it for the Cubic Data Team
                dashboard — check back shortly.
              </p>
            </div>
          </div>
        </div>
      ) : null}

      <OnboardingModal open={modalKind === "onboarding"} onClose={closeOnboarding} />
      <PracticeClassesModal
        open={modalKind === "practice-classes"}
        onClose={closePracticeClasses}
      />
      <DoNotApplyModal open={modalKind === "do-not-apply"} onClose={closeDoNotApply} />
      <ResumeRetargetingPromptModal
        open={modalKind === "resume-prompt"}
        onClose={closeResumePrompt}
      />
      <StatusToast
        toast={status.toast}
        durationMs={SIGN_IN_TOAST_MS}
        onDismiss={status.dismiss}
      />
    </>
  );
}
