import { AuthGate } from "@/components/auth/AuthGate";

export default function InterviewsLayout({ children }: { children: React.ReactNode }) {
  return <AuthGate>{children}</AuthGate>;
}
