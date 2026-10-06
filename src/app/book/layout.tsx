import { AuthGate } from "@/components/auth/AuthGate";

export default function BookLayout({ children }: { children: React.ReactNode }) {
  return <AuthGate>{children}</AuthGate>;
}
