"use client";

import Link from "next/link";
import { useState } from "react";
import { Skel } from "@/components/ui/Skeleton";
import { CalendarSkeleton } from "./CalendarMonth";
import "../book/phone-call/phone-call.css";
import "./attendance.css";

/** Remembered from the last visit so staff get a list-shaped skeleton, candidates a calendar. */
const ROLE_KEY = "otter-attendance-role";

export function rememberRole(isStaff: boolean) {
  try {
    window.localStorage.setItem(ROLE_KEY, isStaff ? "staff" : "candidate");
  } catch {
    /* storage blocked: the calendar skeleton is the default */
  }
}

function useRememberedRole() {
  const [role] = useState<string | null>(() => {
    try {
      return typeof window === "undefined" ? null : window.localStorage.getItem(ROLE_KEY);
    } catch {
      return null;
    }
  });
  return role;
}

/** Staff landing view: the candidate list. */
function ListSkeleton() {
  return (
    <div className="at-stack" aria-busy="true" aria-label="Loading candidates">
      <section className="at-card">
        <div className="at-card-head">
          <div style={{ display: "grid", gap: 8 }}>
            <Skel w={110} h={14} />
            <Skel w={160} h={10} />
          </div>
        </div>
        <div className="at-table-scroll">
          <table className="at-table">
            <thead>
              <tr>
                {["28%", "30%", "18%", "18%"].map((w, i) => (
                  <th key={i}>
                    <Skel w={w === "28%" ? 80 : 70} h={9} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {Array.from({ length: 8 }).map((_, i) => (
                <tr key={i}>
                  <td>
                    <span className="at-person">
                      <Skel w={32} h={32} r={999} />
                      <span style={{ display: "grid", gap: 6 }}>
                        <Skel w={130} h={12} />
                        <Skel w={170} h={9} />
                      </span>
                    </span>
                  </td>
                  <td>
                    <Skel w={70} h={11} />
                    <Skel w={140} h={6} r={999} style={{ marginTop: 8 }} />
                  </td>
                  <td>
                    <Skel w={60} h={11} />
                  </td>
                  <td>
                    <Skel w={80} h={11} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

/** Candidate view: calendar on the left, profile card on the right. */
function CalendarPageSkeleton() {
  return (
    <div className="at-detail" aria-busy="true" aria-label="Loading attendance">
      <div className="at-detail-body">
        <div className="at-card at-cal-card">
          <div className="at-card-head">
            <Skel w={130} h={14} />
            <Skel w={180} h={10} />
          </div>
          <CalendarSkeleton />
        </div>
        <aside className="at-side-col">
          <div className="at-card at-pf">
            <Skel w="60%" h={16} />
            <Skel w="45%" h={11} style={{ marginTop: 8 }} />
            <div className="at-pf-score">
              <Skel w={90} h={34} r={6} />
              <Skel w="70%" h={11} style={{ marginTop: 10 }} />
              <Skel h={6} r={999} style={{ marginTop: 14 }} />
            </div>
            <Skel w={120} h={10} />
            {Array.from({ length: 5 }).map((_, i) => (
              <div key={i} style={{ display: "flex", justifyContent: "space-between", padding: "9px 0" }}>
                <Skel w="50%" h={11} />
                <Skel w={24} h={11} />
              </div>
            ))}
            <Skel h={44} r={10} style={{ marginTop: "auto" }} />
          </div>
        </aside>
      </div>
    </div>
  );
}

/** Loading state shaped like what this viewer will actually see. */
export function AttendanceBodySkeleton({ view }: { view?: "list" | "calendar" } = {}) {
  const role = useRememberedRole();
  const shape = view ?? (role === "staff" ? "list" : "calendar");
  return shape === "list" ? <ListSkeleton /> : <CalendarPageSkeleton />;
}

/** Whole-page skeleton (site header + body), shown while the sign-in check runs. */
export function AttendancePageSkeleton() {
  return (
    <div className="at-root">
      <div className="pca-root at-shell">
        <header className="site-header">
          <div className="topbar">
            <div className="topbar-inner">
              <div className="brand">
                <Link href="/" className="brand-home" aria-label="Go to Cubic Data home">
                  <div className="brand-name">CUBIC</div>
                  <div className="brand-sub">Technologies</div>
                </Link>
              </div>
              <div className="page-title">
                <h1>Otter Attendance</h1>
                <div className="page-subtitle">Mon–Fri · 11:30 AM &amp; 2:00 PM CST</div>
              </div>
              <div className="actions" />
            </div>
          </div>
          <div className="red-rule" />
        </header>
      </div>
      <main className="at-main">
        <AttendanceBodySkeleton />
      </main>
    </div>
  );
}
