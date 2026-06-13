# Runbook — reverse proxy (Nginx Proxy Manager) + notifications ntfy

**Contexte vérifié sur le box (2026-06-13)** : pas de nginx en fichiers — tu utilises
**Nginx Proxy Manager** (conteneur, UI sur `http://192.168.129.85:81`, écoute 80/443).
storm-codex est publié sur `:5102` (→ conteneur `:8088`). ntfy tourne en `:8093` (→ `:80`),
sans `server.yml` (config par défaut). NTFY_URL a été câblé (`http://ntfy:80`) côté Jarvis.

---

## 1. Reverse proxy storm-codex (via Nginx Proxy Manager)

But : servir le site + le widget OBS derrière un nom propre (et TLS), au lieu de `:5102`.

### Étapes dans l'UI NPM (`:81`)
1. **Hosts → Proxy Hosts → Add Proxy Host**.
2. Onglet **Details** :
   - **Domain Names** : ton nom (ex. `storm.hella.lan`, ou un vrai domaine/sous-domaine si tu en
     as un pointé sur le box ; sinon ajoute le nom dans le `hosts` de ta machine → 192.168.129.85).
   - **Scheme** : `http`
   - **Forward Hostname / IP** : `192.168.129.85` (ou le nom du conteneur `storm-codex-server`
     **si** tu attaches NPM au même réseau docker — sinon l'IP du box marche très bien).
   - **Forward Port** : `5102`
   - **Block Common Exploits** : ON
   - **⚠️ Websockets Support : ON** — indispensable : le widget OBS et la liste de matchs se
     mettent à jour en direct via `/ws`. Sans ça, pas de live (il faudra recharger).
3. Onglet **SSL** (recommandé) :
   - **SSL Certificate → Request a new certificate** (Let's Encrypt) si le domaine est public ;
     sinon certificat auto-signé / pas de TLS en LAN pur.
   - **Force SSL** + **HTTP/2** : ON si TLS.
4. **Save**.

### Résultat
- Site : `https://storm.hella.lan/`
- Widget OBS (browser source) : `https://storm.hella.lan/widget?me=matella`
  (transparent, live ; `?me=matella` = ta perspective : V/D, KDA, KP).
- L'API et le WS passent par le même host (`/api`, `/ws`) — rien à configurer de plus côté front
  (tout est en chemins relatifs).

### Notes
- Si tu restes en LAN sans domaine, tu peux continuer à utiliser `http://192.168.129.85:5102`
  directement dans OBS — le reverse proxy n'est utile que pour un nom propre + TLS.
- Pas besoin de toucher au compose de storm-codex : il continue d'exposer `:5102`, NPM tape dessus.

---

## 2. Notifications ntfy (briefs post-game + reminders Jarvis)

Le brief post-game (`[hots_brief]`) et les autres notifs Jarvis sont **publiés** sur le serveur
ntfy (topic `jarvis-62a7e8eba161`). Il restait juste à **s'y abonner** (0 abonné = personne ne
les voit).

### A. S'abonner (le minimum pour recevoir)
- **Appli mobile ntfy** (iOS/Android, gratuite) :
  1. Réglages → Default server → `http://192.168.129.85:8093` (ou l'URL proxifiée, cf. C).
  2. **Subscribe to topic** → `jarvis-62a7e8eba161`.
- **Web** : ouvrir `http://192.168.129.85:8093/jarvis-62a7e8eba161` (laisse l'onglet ouvert).
- **Desktop** : `ntfy subscribe jarvis-62a7e8eba161` (CLI) si tu veux.

> Le topic est dans le `.env` Jarvis du box (`NTFY_TOPIC`). C'est un secret de facto (qui connaît
> le topic peut lire/écrire) — ne le publie pas. Pour durcir, voir D (auth).

### B. (Recommandé) persistance du cache — pour relire l'historique
Par défaut ce ntfy n'a pas de `server.yml` → cache mémoire limité (d'où les briefs qui
« disparaissent »). Pour garder l'historique et que l'appli rattrape les notifs manquées :

Monter un `server.yml` au conteneur ntfy (dans le compose Jarvis, service `ntfy`) :
```yaml
    volumes:
      - ./ntfy/server.yml:/etc/ntfy/server.yml:ro
      - ntfy-cache:/var/cache/ntfy
```
`ntfy/server.yml` :
```yaml
base-url: "http://192.168.129.85:8093"   # ou l'URL proxifiée (cf. C)
cache-file: "/var/cache/ntfy/cache.db"
cache-duration: "168h"                    # 7 jours d'historique
```
puis `docker compose up -d ntfy`. (Ajouter le volume `ntfy-cache` aux volumes nommés.)

### C. (Optionnel) ntfy derrière NPM — requis pour notifs iOS fiables
L'appli iOS exige HTTPS + `base-url` correct. Ajoute un Proxy Host NPM (même méthode qu'au §1) :
- Domain : `ntfy.hella.lan` → Forward `192.168.129.85:8093`, **Websockets Support ON**, SSL ON.
- Mets `base-url: "https://ntfy.hella.lan"` dans `server.yml`, et abonne-toi à ce host dans l'appli.

### D. (Optionnel) authentifier le serveur ntfy (le rendre privé)
Par défaut le serveur est ouvert (qui a l'URL+topic lit/écrit). Pour le fermer :
```yaml
# server.yml
auth-file: "/var/cache/ntfy/auth.db"
auth-default-access: "deny-all"
```
puis créer un user + token et donner ce token à Jarvis (`NTFY_TOKEN` dans le `.env` du box) et à
l'appli. (Jarvis `send()` utilise déjà le topic ; il faudra lui passer le token — petit ajout si
tu veux ce niveau.)

---

## Récap minimal (si tu veux juste que ça marche maintenant)
1. **OBS** : ajoute une *browser source* `http://192.168.129.85:5102/widget?me=matella`
   (taille ~360×90, fond transparent). Le reverse proxy NPM est un plus (nom propre + TLS), pas
   un prérequis.
2. **Briefs** : installe l'appli ntfy, serveur `http://192.168.129.85:8093`, abonne-toi au topic
   `jarvis-62a7e8eba161`. Joue une partie → tu reçois « 🏆/💀 HotS — … — héros k/a/d ».
3. Le reste (TLS, persistance cache, auth) = durcissement optionnel ci-dessus.
