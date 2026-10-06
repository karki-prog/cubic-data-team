import { AuthGate } from "@/components/auth/AuthGate";

export default function HiringLayout({ children }: { children: React.ReactNode }) {
  return <AuthGate>{children}</AuthGate>;
}
