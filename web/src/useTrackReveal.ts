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
