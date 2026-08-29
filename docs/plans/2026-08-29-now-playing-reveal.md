# Now Playing — reveal animé : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ajouter une variante `/now-playing?reveal` qui annonce chaque nouveau morceau par une
grande carte (pochette 264 px, titre, artiste) tenue 2,6 s, puis se replie en fondu sur le badge
compact existant en haut à droite.

**Architecture:** Machine à trois états (`hidden|big|mini`) isolée dans un **réducteur pur**
(`revealState.ts`, testable en environnement node), enveloppée d'un hook mince qui porte la
minuterie de hold, et rendue par un composant de présentation dont le morph est entièrement en CSS
(une boîte qui s'anime en taille, deux couches de contenu en fondu croisé). Le sondage
`/api/now-playing` passe de 5 s à 2 s.

**Tech Stack:** React 18 + TypeScript + Vite, TanStack Query v5, vitest 2 (environnement **node**,
pas de React Testing Library dans ce repo), CSS simple par fichier d'overlay.

**Spec:** `docs/specs/2026-08-29-now-playing-reveal-design.md`
**Maquette animée:** `docs/specs/2026-08-29-now-playing-reveal-mockup.html` (n'anime que servie en
http ; antérieure à la décision « titre deux lignes » — la spec fait foi).

## Global Constraints

- **Ne rien casser des variantes existantes.** `/now-playing` (carte étoffée) et
  `/now-playing?mini` doivent se comporter exactement comme aujourd'hui. La source OBS de
  l'opérateur pointe sur l'une d'elles.
- **Timings figés** : entrée 440 ms, hold 2600 ms, morph 620 ms, easing
  `cubic-bezier(.22,.9,.24,1)`. Ne pas « améliorer » ces valeurs, elles ont été réglées à la main.
- **Géométrie figée** : grand = 300×408 (padding 18, pochette 264×264 rayon 12, bloc de titre
  **fixe 52 px** sur deux lignes) ; mini = 290×68 (padding 10, pochette 48×48 rayon 8) — identique
  à l'existant.
- **Ancrage** : `OverlayFrame anchor="top-right" pad={36}`, inchangé.
- **Préfixe CSS obligatoire `npr-`.** `web/src/theme.css` définit déjà `.card`, `.kick` et `.mono`
  en global ; une classe `.card` non préfixée casserait la carte. Toutes les classes du nouveau
  fichier CSS commencent par `npr-`, sous la racine `.npr`.
- **Conventions repo** : TypeScript strict, commits conventionnels, commentaires et docs **en
  français** comme le reste de `web/src`.
- **Pas de `any`.** Le parseur existant utilise `Record<string, unknown>` + gardes typées ; suivre
  ce style.

---

### Task 1 : Identité de piste

Le reveal se déclenche sur un **changement d'identité de piste**. Aujourd'hui `parseTrack`
n'extrait aucun identifiant : deux morceaux de même titre seraient confondus. On ajoute `id` au
`Track` et une fonction `trackKey` qui en dérive une clé stable, avec repli sur `titre|artiste`.

**Files:**
- Modify: `web/src/api.ts` (interface `Track` ~ligne 490, corps de `parseTrack` ~ligne 503)
- Create: `web/src/api.test.ts`

**Interfaces:**
- Consumes: rien (première tâche).
- Produces:
  - `interface Track` gagne le champ optionnel `id?: string`.
  - `export function trackKey(t: Track): string | null` — `null` si la piste n'a ni `id` ni
    `title`. Consommée par les tâches 2 et 3.

- [ ] **Step 1 : Écrire les tests qui échouent**

Créer `web/src/api.test.ts` :

```ts
import { describe, it, expect } from "vitest";
import { parseTrack, trackKey } from "./api";

describe("parseTrack", () => {
  it("extrait l'id Spotify de la forme /api/playback/now", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { id: "4uLU6hMCjMI75M1A2tKUQC", name: "Ghosts", artists: [{ name: "Foundry" }] } },
    });
    expect(t.id).toBe("4uLU6hMCjMI75M1A2tKUQC");
    expect(t.playing).toBe(true);
  });

  it("retombe sur l'uri quand id est absent", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, track: { uri: "spotify:track:abc", name: "Ghosts" } },
    });
    expect(t.id).toBe("spotify:track:abc");
  });

  it("laisse id indéfini quand la source n'en fournit aucun", () => {
    const t = parseTrack({
      authenticated: true,
      current: { isPlaying: true, current: { name: "Ghosts", artist: "Foundry" } },
    });
    expect(t.id).toBeUndefined();
    expect(t.title).toBe("Ghosts");
  });
});

describe("trackKey", () => {
  it("préfère l'id quand il existe", () => {
    expect(trackKey({ playing: true, id: "abc", title: "Ghosts", artist: "Foundry" })).toBe("abc");
  });

  it("retombe sur titre|artiste sans id", () => {
    expect(trackKey({ playing: true, title: "Ghosts", artist: "Foundry" })).toBe("Ghosts|Foundry");
  });

  it("tolère un artiste manquant", () => {
    expect(trackKey({ playing: true, title: "Ghosts" })).toBe("Ghosts|");
  });

  it("rend null sans id ni titre", () => {
    expect(trackKey({ playing: false })).toBeNull();
  });
});
```

- [ ] **Step 2 : Lancer les tests pour vérifier qu'ils échouent**

Run: `npm test --prefix web -- api.test.ts`
Expected: FAIL — `trackKey` n'est pas exporté par `./api`, et les assertions sur `t.id` échouent.

- [ ] **Step 3 : Ajouter `id` à `Track` et l'extraire dans `parseTrack`**

Dans `web/src/api.ts`, ajouter le champ à l'interface `Track` (juste après `playing`) :

```ts
export interface Track {
  playing: boolean;
  /** identifiant stable de la piste (`id` ou `uri` Spotify) — absent sur l'ancien engine. */
  id?: string;
  title?: string;
  artist?: string;
  art?: string;
  album?: string;
  durationMs?: number;
  progressMs?: number;
}
```

Dans le corps de `parseTrack`, après la ligne qui calcule `const title = …`, ajouter :

```ts
  const id = str(t.id) ?? str(t.uri);
```

puis ajouter `id` à l'objet retourné :

```ts
  return { playing: !!(np?.authenticated && title && isPlaying), id, title, artist, art, album, durationMs, progressMs };
```

- [ ] **Step 4 : Ajouter `trackKey`**

Dans `web/src/api.ts`, juste après `parseTrack` :

```ts
/** Clé d'identité d'une piste pour détecter un changement de morceau. `id` quand la source en
 *  fournit un, sinon `titre|artiste` (l'ancien engine n'a pas d'identifiant). `null` = rien
 *  d'exploitable, donc rien à annoncer. */
export function trackKey(t: Track): string | null {
  if (t.id) return t.id;
  if (t.title) return `${t.title}|${t.artist ?? ""}`;
  return null;
}
```

- [ ] **Step 5 : Lancer les tests pour vérifier qu'ils passent**

Run: `npm test --prefix web -- api.test.ts`
Expected: PASS — 7 tests.

- [ ] **Step 6 : Vérifier que rien n'a régressé**

Run: `npm test --prefix web`
Expected: PASS — toute la suite (`api`, `clipExport`, `replay2d`, `usePlayback`).

- [ ] **Step 7 : Commit**

```bash
git add web/src/api.ts web/src/api.test.ts
git commit -m "feat(now-playing): identité de piste (id Spotify + trackKey)"
```

---

### Task 2 : Réducteur de reveal

Toute la politique de déclenchement vit ici, en pur, sans React ni DOM — c'est la couture qui rend
la logique testable dans l'environnement node de vitest (ce repo n'a pas React Testing Library ;
`usePlayback.ts` / `usePlayback.test.ts` suivent exactement ce modèle avec sa fonction pure
`advance`).

Politique retenue (spec, décisions 2 et 4) : **tout démarrage de lecture** annonce (nouveau
morceau, reprise après pause, premier montage avec de la musique en cours) ; un changement de piste
pendant que la grande carte est affichée **remplace le contenu et relance le hold** sans repasser
par le mini.

**Files:**
- Create: `web/src/revealState.ts`
- Create: `web/src/revealState.test.ts`

Racine de `web/src/` et non `pages/` : c'est là que vivent déjà la logique pure et les hooks du
repo (`usePlayback.ts`, `replay2d.ts`, `clipExport.ts`). Seuls les composants d'overlay et leur CSS
vont dans `pages/` (`DraftOverlay.tsx` + `draft-overlay.css`).

**Interfaces:**
- Consumes: rien de la tâche 1 au niveau du code (le réducteur reçoit déjà une clé calculée).
- Produces :
  - `export type RevealPhase = "hidden" | "big" | "mini"`
  - `export interface RevealState { phase: RevealPhase; key: string | null; holdToken: number }`
  - `export interface RevealInput { playing: boolean; key: string | null }`
  - `export const INITIAL_REVEAL: RevealState`
  - `export function nextRevealState(prev: RevealState, next: RevealInput): RevealState`
  - `export function holdExpired(prev: RevealState): RevealState`

  `holdToken` s'incrémente **à chaque fois que le hold doit être (ré)armé** ; c'est ce compteur que
  le hook de la tâche 3 met en dépendance d'effet pour relancer sa minuterie. Les deux fonctions
  rendent **l'objet `prev` lui-même** (identité référentielle) quand rien ne change, pour que le
  hook ne réarme pas la minuterie à chaque sondage.

- [ ] **Step 1 : Écrire les tests qui échouent**

Créer `web/src/revealState.test.ts` :

```ts
import { describe, it, expect } from "vitest";
import { INITIAL_REVEAL, nextRevealState, holdExpired, type RevealState } from "./revealState";

const at = (phase: RevealState["phase"], key: string | null, holdToken = 0): RevealState =>
  ({ phase, key, holdToken });

describe("nextRevealState", () => {
  it("premier sondage avec de la musique en cours → big, hold armé", () => {
    const s = nextRevealState(INITIAL_REVEAL, { playing: true, key: "a" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("a");
    expect(s.holdToken).toBe(INITIAL_REVEAL.holdToken + 1);
  });

  it("reprise après pause → big (même morceau qu'avant la pause)", () => {
    const paused = nextRevealState(at("mini", "a", 3), { playing: false, key: "a" });
    expect(paused.phase).toBe("hidden");
    const resumed = nextRevealState(paused, { playing: true, key: "a" });
    expect(resumed.phase).toBe("big");
    expect(resumed.holdToken).toBe(paused.holdToken + 1);
  });

  it("changement de piste en big → reste big, contenu remplacé, hold relancé", () => {
    const s = nextRevealState(at("big", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("changement de piste en mini → big", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: "b" });
    expect(s.phase).toBe("big");
    expect(s.key).toBe("b");
    expect(s.holdToken).toBe(6);
  });

  it("arrêt de lecture → hidden, sans incrémenter le hold", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: false, key: "a" });
    expect(s.phase).toBe("hidden");
    expect(s.key).toBeNull();
    expect(s.holdToken).toBe(5);
  });

  it("piste sans clé exploitable → hidden", () => {
    const s = nextRevealState(at("mini", "a", 5), { playing: true, key: null });
    expect(s.phase).toBe("hidden");
  });

  it("sondage identique → même objet, aucun réarmement", () => {
    const prev = at("mini", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("sondage identique en big → même objet (le hold ne redémarre pas)", () => {
    const prev = at("big", "a", 5);
    expect(nextRevealState(prev, { playing: true, key: "a" })).toBe(prev);
  });

  it("arrêt répété → même objet", () => {
    const prev = at("hidden", null, 5);
    expect(nextRevealState(prev, { playing: false, key: null })).toBe(prev);
  });
});

describe("holdExpired", () => {
  it("big → mini", () => {
    expect(holdExpired(at("big", "a", 5))).toEqual(at("mini", "a", 5));
  });

  it("ne touche pas mini", () => {
    const prev = at("mini", "a", 5);
    expect(holdExpired(prev)).toBe(prev);
  });

  it("ne touche pas hidden", () => {
    const prev = at("hidden", null, 5);
    expect(holdExpired(prev)).toBe(prev);
  });
});
```

- [ ] **Step 2 : Lancer les tests pour vérifier qu'ils échouent**

Run: `npm test --prefix web -- revealState.test.ts`
Expected: FAIL — `Failed to resolve import "./revealState"`.

- [ ] **Step 3 : Écrire le réducteur**

Créer `web/src/revealState.ts` :

```ts
// Machine à états du reveal musical (/now-playing?reveal), en pur : ni React ni DOM, donc
// testable dans l'environnement node de vitest. Même découpage que usePlayback.ts / advance().

/** `hidden` = rien à l'écran ; `big` = grande carte d'annonce ; `mini` = badge compact persistant. */
export type RevealPhase = "hidden" | "big" | "mini";

export interface RevealState {
  phase: RevealPhase;
  /** clé de la piste portée par la carte ; `null` quand `phase === "hidden"`. */
  key: string | null;
  /** incrémenté chaque fois que le hold doit être (ré)armé — dépendance d'effet du hook. */
  holdToken: number;
}

export interface RevealInput {
  playing: boolean;
  key: string | null;
}

export const INITIAL_REVEAL: RevealState = { phase: "hidden", key: null, holdToken: 0 };

/**
 * Transition sur une mise à jour de sondage.
 *
 * Règle retenue (spec, décision 2) : **tout démarrage de lecture annonce** — nouveau morceau,
 * reprise après pause, premier montage avec de la musique en cours. Et (décision 4) un changement
 * de piste pendant la grande carte la **recible sur place** : on reste en `big`, on relance le
 * hold, on ne repasse pas par le mini — sinon la carte fait le yoyo quand on enchaîne les skips.
 *
 * Rend `prev` **tel quel** quand rien ne change, pour que le hook ne réarme pas sa minuterie à
 * chaque sondage.
 */
export function nextRevealState(prev: RevealState, next: RevealInput): RevealState {
  if (!next.playing || next.key === null) {
    return prev.phase === "hidden" ? prev : { phase: "hidden", key: null, holdToken: prev.holdToken };
  }
  if (prev.phase === "hidden" || next.key !== prev.key) {
    return { phase: "big", key: next.key, holdToken: prev.holdToken + 1 };
  }
  return prev;
}

/** Transition sur expiration du hold : la grande carte se replie sur le badge. */
export function holdExpired(prev: RevealState): RevealState {
  return prev.phase === "big" ? { ...prev, phase: "mini" } : prev;
}
```

- [ ] **Step 4 : Lancer les tests pour vérifier qu'ils passent**

Run: `npm test --prefix web -- revealState.test.ts`
Expected: PASS — 12 tests.

- [ ] **Step 5 : Commit**

```bash
git add web/src/revealState.ts web/src/revealState.test.ts
git commit -m "feat(now-playing): réducteur pur de la machine à états du reveal"
```

---

### Task 3 : Hook, carte, route et vérification live

Le hook, le CSS et le composant ne sont observables qu'une fois routés : ils forment un seul
livrable, vérifié à l'écran. Pas de test unitaire du hook — ce repo n'a pas React Testing Library
et l'introduire pour un hook de six lignes serait disproportionné ; toute la logique qui mérite un
test est déjà couverte en tâche 2.

**Files:**
- Create: `web/src/useTrackReveal.ts`
- Create: `web/src/pages/now-playing-reveal.css`
- Create: `web/src/pages/RevealCard.tsx`
- Modify: `web/src/pages/NowPlaying.tsx` (aiguillage de variante + `refetchInterval`)
- Modify: `docs/STATUS.md`

**Interfaces:**
- Consumes:
  - `trackKey(t: Track): string | null` et `Track` (avec `id?`) — tâche 1, depuis `../api`.
  - `INITIAL_REVEAL`, `nextRevealState`, `holdExpired`, `RevealPhase` — tâche 2, depuis
    `./revealState` (dans `useTrackReveal.ts`, à la racine) ou `../revealState` (dans
    `RevealCard.tsx`, sous `pages/`).
  - `OverlayFrame` — existant, `../components/OverlayFrame`.
- Produces:
  - `export function useTrackReveal(playing: boolean, key: string | null, holdMs?: number): RevealPhase`
  - `export function RevealCard({ track, phase }: { track: Track; phase: RevealPhase }): JSX.Element`

- [ ] **Step 1 : Écrire le hook**

Créer `web/src/useTrackReveal.ts` :

```ts
// Minuterie du reveal musical. Le hook ne porte aucune règle : il applique le réducteur pur de
// revealState.ts et arme le hold. `holdToken` en dépendance d'effet fait que tout (re)ciblage
// annule la minuterie en cours et en repart une neuve.
import { useEffect, useState } from "react";
import { INITIAL_REVEAL, nextRevealState, holdExpired, type RevealPhase } from "./revealState";

/** Durée d'affichage de la grande carte avant repli sur le badge (spec : figée à 2,6 s). */
export const HOLD_MS = 2600;

export function useTrackReveal(playing: boolean, key: string | null, holdMs = HOLD_MS): RevealPhase {
  const [state, setState] = useState(INITIAL_REVEAL);

  useEffect(() => {
    setState((prev) => nextRevealState(prev, { playing, key }));
  }, [playing, key]);

  useEffect(() => {
    if (state.phase !== "big") return;
    const id = setTimeout(() => setState(holdExpired), holdMs);
    return () => clearTimeout(id);
  }, [state.phase, state.holdToken, holdMs]);

  return state.phase;
}
```

- [ ] **Step 2 : Écrire le CSS du morph**

Créer `web/src/pages/now-playing-reveal.css`. **Toutes les classes sont préfixées `npr-`** :
`theme.css` définit `.card`, `.kick` et `.mono` en global.

```css
/* Overlay musique OBS — variante « reveal » (/now-playing?reveal).
   Morph « Dissolve » : une seule boîte s'anime en taille, deux couches de contenu se fondent
   l'une dans l'autre. Timings figés par la spec 2026-08-29 — ne pas retoucher.
   Porté de docs/specs/2026-08-29-now-playing-reveal-mockup.html (option A). */
.npr {
  --npr-morph: 620ms;
  --npr-enter: 440ms;
  --npr-ease: cubic-bezier(.22, .9, .24, 1);
}

.npr-card {
  position: relative;
  background: linear-gradient(135deg, rgba(18, 20, 28, .96), rgba(12, 13, 18, .96));
  border: 1px solid var(--hairline-strong);
  box-shadow: 0 14px 40px rgba(0, 0, 0, .55);
  will-change: width, height, transform, opacity;
  /* état de repos = géométrie du badge mini ; l'entrée grandit depuis cette taille. */
  width: 290px;
  height: 68px;
  border-radius: 16px;
  opacity: 0;
  transform: translateY(-10px) scale(.94);
  transition:
    width var(--npr-morph) var(--npr-ease),
    height var(--npr-morph) var(--npr-ease),
    border-radius var(--npr-morph) var(--npr-ease),
    opacity var(--npr-enter) ease,
    transform var(--npr-enter) var(--npr-ease);
}

.npr-layer { position: absolute; inset: 0; opacity: 0; transition: opacity calc(var(--npr-morph) * .55) ease; }

/* ── couche « grande carte » ─────────────────────────────────────────────── */
.npr-big { padding: 18px; display: flex; flex-direction: column; align-items: center; text-align: center; }
.npr-big .npr-krow { display: flex; align-items: center; gap: 8px; align-self: flex-start; height: 12px; margin-bottom: 10px; }
.npr-big .npr-art { width: 264px; height: 264px; border-radius: 12px; }
.npr-big .npr-title {
  margin-top: 14px; height: 52px; font-size: 20px; line-height: 1.3; font-weight: 700; width: 100%;
  /* deux lignes maxi : 264px à 20px tronquent vers 22 caractères sur une seule ligne. La hauteur
     reste fixe pour que la boîte garde une cible d'animation. */
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
}
.npr-big .npr-artist { margin-top: 2px; height: 18px; font-size: 14px; color: var(--text-2); width: 100%;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* ── couche « badge mini » (géométrie identique à la variante ?mini) ─────── */
.npr-mini { padding: 10px; display: flex; gap: 12px; align-items: center; }
.npr-mini .npr-art { width: 48px; height: 48px; border-radius: 8px; }
.npr-mini .npr-txt { flex: 1; min-width: 0; }
.npr-mini .npr-title { font-size: 14px; font-weight: 700; line-height: 1.25;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.npr-mini .npr-artist { margin-top: 1px; font-size: 12px; color: var(--text-2);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* ── pochette et placeholder ─────────────────────────────────────────────── */
.npr-art { background-size: cover; background-position: center; flex-shrink: 0; box-shadow: 0 4px 14px rgba(0, 0, 0, .5); }
.npr-art.npr-none { display: flex; align-items: center; justify-content: center;
  background: var(--surface-2); color: var(--text-2); }
.npr-big .npr-art.npr-none { font-size: 100px; }
.npr-mini .npr-art.npr-none { font-size: 18px; }

/* ── kicker + égaliseur ──────────────────────────────────────────────────── */
.npr-kick { color: var(--accent); font-size: 10px; letter-spacing: .08em; text-transform: uppercase; font-weight: 600; }
.npr-eq { display: inline-flex; align-items: flex-end; gap: 2px; height: 11px; }
.npr-eq i { width: 3px; background: var(--accent); border-radius: 1px; animation: npr-eq .9s ease-in-out infinite; }
.npr-eq i:nth-child(2) { animation-delay: .18s; }
.npr-eq i:nth-child(3) { animation-delay: .36s; }
.npr-eq i:nth-child(4) { animation-delay: .54s; }
@keyframes npr-eq { 0%, 100% { height: 3px; } 50% { height: 11px; } }

/* ── états ───────────────────────────────────────────────────────────────── */
.npr.is-big .npr-card { width: 300px; height: 408px; border-radius: 18px; opacity: 1; transform: none; }
.npr.is-big .npr-big { opacity: 1; transition-delay: calc(var(--npr-morph) * .35); }
.npr.is-mini .npr-card { width: 290px; height: 68px; border-radius: 16px; opacity: 1; transform: none; }
.npr.is-mini .npr-mini { opacity: 1; transition-delay: calc(var(--npr-morph) * .45); }

@media (prefers-reduced-motion: reduce) {
  .npr-card, .npr-layer { transition: none; }
  .npr-eq i { animation: none; height: 7px; }
}
```

- [ ] **Step 3 : Écrire le composant de présentation**

Créer `web/src/pages/RevealCard.tsx` :

```tsx
import type { Track } from "../api";
import type { RevealPhase } from "../revealState";
import "./now-playing-reveal.css";

/** Pochette, ou placeholder ♫ quand la source n'en fournit pas. */
function Art({ art }: { art?: string }) {
  if (!art) return <div className="npr-art npr-none">♫</div>;
  return <div className="npr-art" style={{ backgroundImage: `url(${art})` }} />;
}

/**
 * Carte du reveal musical : une seule boîte qui s'anime entre la grande annonce et le badge
 * compact, avec les deux couches de contenu en fondu croisé. Les deux couches sont **toujours
 * montées** — c'est ce qui permet le fondu ; c'est la classe d'état sur la racine qui décide.
 */
export function RevealCard({ track, phase }: { track: Track; phase: RevealPhase }) {
  return (
    <div className={`npr is-${phase}`}>
      <div className="npr-card">
        <div className="npr-layer npr-big">
          <div className="npr-krow">
            <span className="npr-kick">Now playing</span>
            <span className="npr-eq"><i /><i /><i /><i /></span>
          </div>
          <Art art={track.art} />
          <div className="npr-title">{track.title}</div>
          <div className="npr-artist">{track.artist}</div>
        </div>
        <div className="npr-layer npr-mini">
          <Art art={track.art} />
          <div className="npr-txt">
            <div className="npr-title">{track.title}</div>
            <div className="npr-artist">{track.artist}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4 : Brancher la variante et resserrer le sondage**

Dans `web/src/pages/NowPlaying.tsx` :

1. Ajouter les imports en tête du fichier :

```ts
import { parseTrack, trackKey } from "../api";
import { RevealCard } from "./RevealCard";
import { useTrackReveal } from "../useTrackReveal";
```

(la ligne `import { parseTrack } from "../api";` existante est remplacée par celle ci-dessus)

2. Dans le corps de `NowPlaying`, remplacer la ligne de lecture du paramètre d'URL :

```ts
  const params = new URLSearchParams(window.location.search);
  const mini = params.has("mini");
  const reveal = params.has("reveal");
```

3. Passer `refetchInterval` de `5000` à `2000` dans le `useQuery` (spec, décision 3 : sous la règle
   « tout démarrage de lecture annonce », 5 s font arriver le reveal visiblement en retard).

4. Juste après `const t = parseTrack(data);`, appeler le hook — **inconditionnellement**, avant
   tout `return` anticipé, sinon on viole les règles des hooks :

```ts
  const phase = useTrackReveal(t.playing, trackKey(t));
```

5. Ajouter la branche de variante **avant** le `if (!t.playing) return …` existant (la carte doit
   pouvoir rester montée pendant que le CSS la fait disparaître) :

```tsx
  if (reveal) {
    return (
      <OverlayFrame anchor="top-right" pad={36}>
        <RevealCard track={t} phase={phase} />
      </OverlayFrame>
    );
  }
```

6. Mettre à jour le commentaire de tête du fichier pour mentionner les trois variantes :

```
 *   - défaut  : carte étoffée (pochette, titre/artiste/album, progression, égaliseur).
 *   - `?mini` : compacte (pochette + titre + artiste seulement).
 *   - `?reveal` : annonce en grand à chaque démarrage de lecture, puis repli sur `?mini`.
```

- [ ] **Step 5 : Vérifier types et build**

Run: `npm run build --prefix web`
Expected: PASS — `tsc -b` sans erreur puis build Vite. En cas d'erreur « React Hook called
conditionally », c'est que l'appel de `useTrackReveal` a été placé après un `return` : le remonter.

- [ ] **Step 6 : Vérifier que la suite de tests passe toujours**

Run: `npm test --prefix web`
Expected: PASS — suite complète, dont `api.test.ts` et `revealState.test.ts`.

- [ ] **Step 7 : Vérification visuelle en direct**

Démarrer le serveur de dev via l'outil de preview (config `storm-codex-web` de
`.claude/launch.json`, port 5180) — **pas via Bash**.

Orpheus n'étant pas joignable depuis le Mac, `/api/now-playing` répond en erreur ou
`{authenticated:false}` : la carte reste masquée. Piloter donc la machine à états à la main depuis
la console de la page, à `http://localhost:5180/now-playing?reveal` :

```js
// forcer les trois états pour contrôler géométrie et fondu
document.querySelector(".npr").className = "npr is-big";
document.querySelector(".npr").className = "npr is-mini";
```

Vérifier :
- [ ] `is-big` → carte 300×408, ancrée à 36 px des bords haut et droit ;
- [ ] `is-mini` → carte 290×68, même ancrage, coin haut-droit inchangé ;
- [ ] la transition entre les deux dure ~620 ms sans saut de position ;
- [ ] un titre long (injecter `document.querySelector(".npr-big .npr-title").textContent = "Un titre volontairement très long pour tester le passage à la ligne"`) s'affiche sur **deux lignes** puis ellipse, **sans changer la hauteur** de la carte ;
- [ ] sans pochette, le placeholder ♫ s'affiche dans les deux tailles.

Prendre une capture d'écran de l'état `is-big` et de l'état `is-mini` comme preuve.

- [ ] **Step 8 : Vérifier la non-régression des deux autres variantes**

Ouvrir `http://localhost:5180/now-playing` puis `http://localhost:5180/now-playing?mini`.
Expected: cadre vide dans les deux cas (Orpheus injoignable depuis le Mac → `playing:false`), aucune
erreur dans la console. C'est le comportement d'aujourd'hui, inchangé.

- [ ] **Step 9 : Mettre à jour `docs/STATUS.md`**

Dans la section « Scène OBS « entre les games » », remplacer la puce existante :

```
- **`/now-playing`** (source persistante, EN) : widget musique lisant **Orpheus** via proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Affiche « Music — off » tant que Spotify dormant.
```

par :

```
- **`/now-playing`** (source persistante, EN) : widget musique lisant **Orpheus** via proxy
  `/api/now-playing` (config `ORPHEUS_URL`). Affiche « Music — off » tant que Spotify dormant.
  Trois variantes : défaut (carte étoffée), `?mini` (badge compact), `?reveal` (annonce en grand
  à chaque démarrage de lecture — entrée 440 ms, hold 2,6 s, morph 620 ms — puis repli sur le
  badge). Spec `docs/specs/2026-08-29-now-playing-reveal-design.md`, plan
  `docs/plans/2026-08-29-now-playing-reveal.md`. Sondage passé de 5 s à 2 s.
  **Reste à vérifier sur le box** : (a) qu'Orpheus ne relaie pas l'API Spotify sans cache — le
  sondage à 2 s fait ~30 requêtes/min par source ouverte ; repli documenté = push WS
  `music.changed` ; (b) que le reveal s'affiche réellement, ce qui suppose l'OAuth Spotify faite
  (voir la section « Orpheus (musique) » plus bas). Jusque-là seule la géométrie est vérifiée,
  en pilotant les classes d'état à la main.
```

- [ ] **Step 10 : Commit**

```bash
git add web/src/useTrackReveal.ts web/src/pages/now-playing-reveal.css \
        web/src/pages/RevealCard.tsx web/src/pages/NowPlaying.tsx docs/STATUS.md
git commit -m "feat(now-playing): variante ?reveal (grande annonce → badge mini)"
```

---

## Reste à vérifier sur le box (hors plan)

Ces deux points ne peuvent pas être vérifiés depuis le Mac et ne bloquent pas l'implémentation :

1. **Cache Orpheus.** Le passage à 2 s suppose qu'Orpheus ne relaie pas l'API Spotify sans cache.
   Repli documenté si l'hypothèse tombe : push WS `music.changed` (hors scope de la spec).
2. **Rendu réel.** Le reveal ne peut être vu de bout en bout qu'avec l'OAuth Spotify faite côté
   Orpheus (`docs/STATUS.md`, section « Orpheus (musique) »). Jusque-là, seule la géométrie est
   vérifiable, en pilotant les classes à la main.
