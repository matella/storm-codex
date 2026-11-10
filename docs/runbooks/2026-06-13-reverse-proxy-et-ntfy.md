# Runbook — reverse proxy (Nginx Proxy Manager) + ntfy notifications

**Context checked on the box (2026-06-13)**: no nginx in files — you use
**Nginx Proxy Manager** (container, UI on `http://192.168.129.85:81`, listens on 80/443).
storm-codex is published on `:5102` (→ container `:8088`). ntfy runs on `:8093` (→ `:80`),
without `server.yml` (default config). NTFY_URL has been wired (`http://ntfy:80`) on the Jarvis side.

---

## 1. storm-codex reverse proxy (via Nginx Proxy Manager)

Goal: serve the site + the OBS widget behind a proper name (and TLS), instead of `:5102`.

### Steps in the NPM UI (`:81`)
1. **Hosts → Proxy Hosts → Add Proxy Host**.
2. **Details** tab:
   - **Domain Names**: your name (e.g. `storm.hella.lan`, or a real domain/subdomain if you
     have one pointed at the box; otherwise add the name to your machine's `hosts` → 192.168.129.85).
   - **Scheme**: `http`
   - **Forward Hostname / IP**: `192.168.129.85` (or the container name `storm-codex-server`
     **if** you attach NPM to the same docker network — otherwise the box's IP works fine).
   - **Forward Port**: `5102`
   - **Block Common Exploits**: ON
   - **⚠️ Websockets Support: ON** — essential: the OBS widget and the match list
     update live via `/ws`. Without it, no live updates (you will have to reload).
3. **SSL** tab (recommended):
   - **SSL Certificate → Request a new certificate** (Let's Encrypt) if the domain is public;
     otherwise a self-signed certificate / no TLS on a pure LAN.
   - **Force SSL** + **HTTP/2**: ON if TLS.
4. **Save**.

### Result
- Site: `https://storm.hella.lan/`
- OBS widget (browser source): `https://storm.hella.lan/widget?me=matella`
  (transparent, live; `?me=matella` = your perspective: W/L, KDA, KP).
- The API and the WS go through the same host (`/api`, `/ws`) — nothing more to configure on the front-end
  (everything uses relative paths).

### Notes
- If you stay on the LAN without a domain, you can keep using `http://192.168.129.85:5102`
  directly in OBS — the reverse proxy is only useful for a proper name + TLS.
- No need to touch the storm-codex compose: it keeps exposing `:5102`, NPM hits it.

---

## 2. ntfy notifications (post-game briefs + Jarvis reminders)

The post-game brief (`[hots_brief]`) and the other Jarvis notifications are **published** to the ntfy
server (topic `jarvis-62a7e8eba161`). All that was left was to **subscribe to it** (0 subscribers = nobody
sees them).

### A. Subscribe (the minimum to receive)
- **ntfy mobile app** (iOS/Android, free):
  1. Settings → Default server → `http://192.168.129.85:8093` (or the proxied URL, see C).
  2. **Subscribe to topic** → `jarvis-62a7e8eba161`.
- **Web**: open `http://192.168.129.85:8093/jarvis-62a7e8eba161` (leave the tab open).
- **Desktop**: `ntfy subscribe jarvis-62a7e8eba161` (CLI) if you want.

> The topic is in the box's Jarvis `.env` (`NTFY_TOPIC`). It is a de facto secret (whoever knows
> the topic can read/write) — do not publish it. To harden, see D (auth).

### B. (Recommended) cache persistence — to read the history again
By default this ntfy has no `server.yml` → limited memory cache (hence the briefs that
"disappear"). To keep the history and let the app catch up on missed notifications:

Mount a `server.yml` into the ntfy container (in the Jarvis compose, `ntfy` service):
```yaml
    volumes:
      - ./ntfy/server.yml:/etc/ntfy/server.yml:ro
      - ntfy-cache:/var/cache/ntfy
```
`ntfy/server.yml`:
```yaml
base-url: "http://192.168.129.85:8093"   # or the proxied URL (see C)
cache-file: "/var/cache/ntfy/cache.db"
cache-duration: "168h"                    # 7 days of history
```
then `docker compose up -d ntfy`. (Add the `ntfy-cache` volume to the named volumes.)

### C. (Optional) ntfy behind NPM — required for reliable iOS notifications
The iOS app requires HTTPS + a correct `base-url`. Add an NPM Proxy Host (same method as in §1):
- Domain: `ntfy.hella.lan` → Forward `192.168.129.85:8093`, **Websockets Support ON**, SSL ON.
- Set `base-url: "https://ntfy.hella.lan"` in `server.yml`, and subscribe to that host in the app.

### D. (Optional) authenticate the ntfy server (make it private)
By default the server is open (whoever has the URL+topic reads/writes). To close it:
```yaml
# server.yml
auth-file: "/var/cache/ntfy/auth.db"
auth-default-access: "deny-all"
```
then create a user + token and give that token to Jarvis (`NTFY_TOKEN` in the box's `.env`) and to
the app. (Jarvis `send()` already uses the topic; it will need to be passed the token — a small addition if
you want this level.)

---

## Minimal recap (if you just want it to work now)
1. **OBS**: add a *browser source* `http://192.168.129.85:5102/widget?me=matella`
   (size ~360×90, transparent background). The NPM reverse proxy is a plus (proper name + TLS), not
   a prerequisite.
2. **Briefs**: install the ntfy app, server `http://192.168.129.85:8093`, subscribe to the topic
   `jarvis-62a7e8eba161`. Play a game → you receive "🏆/💀 HotS — … — hero k/a/d".
3. The rest (TLS, cache persistence, auth) = optional hardening above.
