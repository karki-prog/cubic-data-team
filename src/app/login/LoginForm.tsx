"use client";

import Link from "next/link";
import { useMemo } from "react";
import { useSearchParams } from "next/navigation";
import "./login.css";

function GoogleMark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 48 48" aria-hidden>
      <path
        fill="#EA4335"
        d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"
      />
      <path
        fill="#4285F4"
        d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"
      />
      <path
        fill="#FBBC05"
        d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"
      />
      <path
        fill="#34A853"
        d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"
      />
    </svg>
  );
}

function messageForError(code: string): string {
  if (!code) return "";
  switch (code) {
    case "not-allowed":
      return "This account is not approved to access the dashboard. Sign in with the personal email you gave the Cubic team, or ask your POC to add you to the access list.";
    case "google-not-configured":
      return "Google sign-in is not configured on this server. Contact an admin.";
    case "invalid-oauth-state":
      return "Your sign-in link expired. Start again with the button below.";
    case "google-cancelled":
      return "Sign-in was cancelled. Try again when you are ready.";
    default:
      return code;
  }
}

export default function LoginForm() {
  const params = useSearchParams();
  const next = params.get("next") || "/";
  const urlError = params.get("error") || "";
  const nextPath = next.startsWith("/") ? next : "/";

  const message = useMemo(() => messageForError(urlError), [urlError]);

  // Native GET form so Next.js never treats this as an in-app route.
  const googleAction =
    process.env.NODE_ENV === "development"
      ? "http://localhost:3000/api/auth/google"
      : "/api/auth/google";

  return (
    <main className="cubic-login">
      <div className="cubic-login-stage">
      <div className="cubic-login-card">
        <div className="cubic-login-head">
          <GoogleMark className="cubic-login-g" />
          <h1 className="cubic-login-title">Sign in</h1>
          <p className="cubic-login-sub">to continue to Cubic Data</p>
        </div>

        <div className="cubic-login-content">
          {message ? (
            <p className="cubic-login-message" role="alert">
              {message}
            </p>
          ) : null}

          <form action={googleAction} method="get" className="cubic-login-form">
            <input type="hidden" name="next" value={nextPath} />
            <button type="submit" className="cubic-login-google">
              <GoogleMark className="cubic-login-google-icon" />
              <span>Continue with Google</span>
            </button>
          </form>

          <p className="cubic-login-note">
            Access is limited to approved Cubic team members. Use the personal
            email you provided to the Cubic team.
          </p>

          <div className="cubic-login-actions">
            <Link href="/" className="cubic-login-back">
              <svg
                className="cubic-login-back-icon"
                viewBox="0 0 16 16"
                aria-hidden
              >
                <path
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M10 3.5 5.5 8l4.5 4.5"
                />
              </svg>
              <span>Back to home</span>
            </Link>
          </div>
        </div>
      </div>
      </div>

      <footer className="cubic-login-footer">
        © {new Date().getFullYear()} Cubic Technologies · All rights reserved
      </footer>
    </main>
  );
}
