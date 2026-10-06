import { DashboardGrid } from "@/components/dashboard/DashboardGrid";
import { SessionBar } from "@/components/dashboard/SessionBar";
import { SiteFooter } from "@/components/ui/SiteFooter";

export default function HomePage() {
  return (
    <main className="cubic-dashboard">
      <header className="cubic-dashboard-header border-b border-[var(--line)] bg-white shadow-[0_1px_0_rgba(0,0,0,0.06)]">
        <div className="cubic-dash-header">
          <h1 className="cubic-dash-title">CUBIC DATA DASHBOARD</h1>
          <div className="cubic-dash-auth">
            <SessionBar />
          </div>
        </div>
        <div className="h-[2px] bg-[var(--red)]" />
      </header>

      <div className="cubic-dashboard-scroll">
        <div className="mx-auto max-w-6xl px-4 py-6 sm:px-8 sm:py-10">
          <DashboardGrid />
        </div>
        <SiteFooter variant="bar" />
      </div>
    </main>
  );
}
