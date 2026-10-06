# ONLYOFFICE Docs — how Cubic uses it

The Document Server is **already self-hosted on the VPS**. It was set up for the
scrapper stack (`~/Desktop/scrapper/Karki_Scrapper/deploy/godaddy/`), which is
why its Docker volumes are named `godaddy_onlyoffice_*`. Cubic reuses that same
instance rather than standing up a second one.

## Verified live layout

| | |
|---|---|
| Container | `onlyoffice-documentserver` (`onlyoffice/documentserver:latest`) |
| Bound on | `127.0.0.1:8082` → container `:80` |
| Public URL | `https://scrapper.yalabyte.com/office-ds/` (via `scrapper-domain.conf`) |
| JWT | **enabled** — `JWT_ENABLED=true`, `JWT_HEADER=Authorization`, inbox + outbox |
| Host from container | `172.17.0.1` / `host.docker.internal` |

Checked: `GET /office-ds/healthcheck` → `true`, `POST /office-ds/docbuilder` → `200`.

> The `office.scrapper.yalabyte.com` vhost in `sites-available/onlyoffice` is
> **not** symlinked into `sites-enabled` — it is dormant. The live route is
> `/office-ds/`. Note `grep -r` over `sites-enabled` finds nothing because those
> entries are symlinks; grep `sites-available` instead.

## The one thing Cubic has to add

`cubic-backend` binds `127.0.0.1:3010`. A container cannot reach the host's
loopback, so the Document Server cannot fetch templates from, or call back into,
Cubic as things stand.

The scrapper hit exactly this and solved it with an nginx block listening on a
port the docker bridge can reach (`:5556` → `127.0.0.1:5555`).
`nginx-cubic-oo-callback.conf` here is the Cubic equivalent on **`:5557` →
`127.0.0.1:3010`**, firewalled to `docker0` only.

## Gotchas already paid for by the scrapper

- **`network_mode: bridge` + `extra_hosts: host.docker.internal:host-gateway`.**
  Compose's own project network (172.18/16) cannot hairpin back to the host under
  ufw. Get this wrong and downloads/callbacks fail and the editor renders blank.
- **JWT signs both directions.** The editor config must be signed
  (`config.token`), and callbacks arrive signed — an unsigned callback while a
  secret is configured must be rejected, or anyone can forge a save.
- **Host allowlist on any URL the server fetches.** The scrapper restricts
  callback/save-as fetches to the configured Document Server host. Without it the
  callback is an SSRF hole.

Reference implementation: `Karki_Scrapper/backend/app/blueprints/documents.py`.

## Local copy (optional)

`docker-compose.onlyoffice.yml` runs an instance on this Mac on the same
`127.0.0.1:8082`, so no code has to branch. The image has a native `linux/arm64`
build, so no emulation on Apple Silicon.

It needs ~3 GB free disk (1.31 GB download, ~2.9 GB unpacked) and a Docker
runtime — this Mac uses **colima**, not Docker Desktop:

```bash
colima start --cpu 4 --memory 4
docker compose -f deploy/onlyoffice/docker-compose.onlyoffice.yml up -d
curl -s http://127.0.0.1:8082/healthcheck   # -> true
```

The local `JWT_SECRET` is deliberately **not** the production secret; it must
match `ONLYOFFICE_JWT_SECRET` in `.env.local`.

Developing against the VPS instance instead is also fine — point
`ONLYOFFICE_URL` at `https://scrapper.yalabyte.com/office-ds` and use the
production secret. That avoids the local disk cost, but shares one Document
Server between dev and prod.
