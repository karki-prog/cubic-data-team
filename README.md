# Cubic Data Dashboard

**Rust is the only public HTTP server.** It owns every `/api/*` request, Google OAuth, cookies, and (in production) the static UI. Next.js is a UI compiler only — no API routes, no middleware, no server-side business logic.

```
src/                   Next.js UI (static export)
  app/                 Pages (login, book, hiring)
  components/          Dashboard, modals, AuthGate (client session check)
  lib/                 Display helpers and types
backend/              Axum — API + UI proxy/static files
scripts/dev.mjs        Rust :3000 (public) + Next :3001 (internal HMR)
```

## Run

Nothing is deployed until you ask.

```bash
cp .env.example .env.local
# set AUTH_JWT_SECRET and Google OAuth if used
npm run dev
```

Open **http://localhost:3000** — that is the Rust process. Next.js is bound to `127.0.0.1:3001` only so Rust can proxy UI + hot reload.

Production-style (after `npm run build`, which writes `out/`):

```bash
npm run build
npm run build:backend
RUST_BACKEND_PORT=3000 cargo run --release --manifest-path backend/Cargo.toml
```

## Auth

- Google sign-in is handled by Rust (`/api/auth/google` + `/callback`)
- Access JWT (15 min) and refresh JWT (7 days) in **httpOnly** cookies, issued by Rust
- The browser checks `/api/auth/me` before showing staff pages
- Cron routes accept `Authorization: Bearer $CRON_SECRET`

Never commit `service-account-key.json`, `gmail-oauth.json`, or `.env.local`.

How https://cubic-data.com writes to Sheets, Drive, Calendar, and mail: **[docs/DATA-FLOW.md](docs/DATA-FLOW.md)**.
