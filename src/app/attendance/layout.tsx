import { AuthGate } from "@/components/auth/AuthGate";
import { AttendancePageSkeleton } from "./AttendanceSkeleton";

export default function AttendanceLayout({ children }: { children: React.ReactNode }) {
  return <AuthGate fallback={<AttendancePageSkeleton />}>{children}</AuthGate>;
}
