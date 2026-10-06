import { Suspense } from "react";
import type { Metadata } from "next";
import LoginForm from "./LoginForm";

export const metadata: Metadata = {
  title: "Sign in — Cubic Data Dashboard",
};

export default function LoginPage() {
  return (
    <Suspense fallback={<main className="min-h-dvh bg-[#f0f4f9]" />}>
      <LoginForm />
    </Suspense>
  );
}
