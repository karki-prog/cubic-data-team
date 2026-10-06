"use client";

import { useEffect, useState } from "react";

type User = { email: string; name: string };

function displayName(name: string, email: string) {
  const raw = String(name || "").trim();
  if (raw.includes(" ")) return raw;
  const local = (raw || email.split("@")[0] || "").replace(/[._]+/g, " ");
  return local.replace(/\b\w/g, (c) => c.toUpperCase());
}

const authBtnClass = "cubic-auth-btn bg-[var(--red)] text-white hover:bg-[var(--red-dark)]";

export function SessionBar({ variant = "header" }: { variant?: "header" | "drawer" }) {
  const [user, setUser] = useState<User | null>(null);

  useEffect(() => {
    fetch("/api/auth/me", { credentials: "same-origin" })
      .then((r) => r.json())
      .then((data: { ok?: boolean; user?: User }) => {
        if (data.ok && data.user) setUser(data.user);
      })
      .catch(() => {});
  }, []);

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST", credentials: "same-origin" });
    window.location.href = "/";
  }

  if (!user) {
    return (
      <a href="/login?next=/" className={authBtnClass}>
        Sign in
      </a>
    );
  }

  const name = displayName(user.name, user.email);

  if (variant === "drawer") {
    return (
      <div className="cubic-drawer-session">
        <span className="cubic-drawer-session-name">{name}</span>
        <button type="button" onClick={logout} className={authBtnClass}>
          Sign out
        </button>
      </div>
    );
  }

  return (
    <div className="flex max-w-full items-center justify-end gap-2 sm:gap-3">
      <span className="hidden max-w-[10rem] truncate text-[13px] font-semibold text-black lg:inline sm:max-w-[14rem] sm:text-[14px]">
        {name}
      </span>
      <button type="button" onClick={logout} className={authBtnClass}>
        Sign out
      </button>
    </div>
  );
}
