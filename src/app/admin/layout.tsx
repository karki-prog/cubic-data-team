import { AuthGate } from "@/components/auth/AuthGate";
import { SuperadminGate } from "@/components/auth/SuperadminGate";
import "./admin.css";

/** Signed in (AuthGate) → superadmin (SuperadminGate) → page. */
export default function AdminLayout({ children }: { children: React.ReactNode }) {
  return (
    <AuthGate>
      <SuperadminGate>{children}</SuperadminGate>
    </AuthGate>
  );
}
