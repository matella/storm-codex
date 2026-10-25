import type { DraftOrder, DraftStep, DraftStepAction } from "./api";

/** Une phase = bloc d'étapes consécutives du même rôle d'ordre (même équipe) : l'unité de lecture
 *  du draft depuis un replay (identique à `DraftState::current_phase_len` côté serveur). */
export interface Phase {
  start: number;
  len: number;
  action: DraftStepAction;
  order: DraftOrder;
}

export function phasesOf(steps: DraftStep[]): Phase[] {
  const out: Phase[] = [];
  steps.forEach((s, i) => {
    const last = out[out.length - 1];
    if (last && last.order === s.order) last.len++;
    else out.push({ start: i, len: 1, action: s.action, order: s.order });
  });
  return out;
}

/** Index de la phase qui sera révélée au prochain « Suivant » (−1 si le draft est terminé). */
export function currentPhase(phases: Phase[], cursor: number): number {
  return phases.findIndex((p) => cursor >= p.start && cursor < p.start + p.len);
}

export function phaseLabel(p: Phase): string {
  return p.len > 1 ? `${p.action} ×${p.len}` : p.action;
}

/** Secondes restantes avant le prochain tick de lecture auto (0 si pas de lecture auto). */
export function secondsLeft(nextAtMs: number | null | undefined, now: number): number {
  if (nextAtMs == null) return 0;
  return Math.max(0, Math.ceil((nextAtMs - now) / 1000));
}

/** Message lisible pour un code d'erreur du chargement d'un replay. */
export function loadErrorMessage(code: string | undefined): string {
  switch (code) {
    case "not_custom": return "not a custom game — only custom games are accepted";
    case "no_draft": return "no draft found in this replay";
    case "parse_failed": return "replay could not be read";
    case "empty_body": return "empty file";
    default: return "upload failed";
  }
}
