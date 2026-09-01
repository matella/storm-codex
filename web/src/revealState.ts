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
