// Tick de rotation auto du diaporama, calculé depuis l'ancre serveur (`slide.since_ms`) : l'overlay OBS
// et l'aperçu de la régie partagent ainsi la même slide, sans minuterie locale qui dérive.
import { useEffect, useState } from "react";
import type { CastSeries } from "../castApi";
import { slideTick } from "../castState";

/** Rafraîchit le tick toutes les 250 ms (bascule à la fin de chaque intervalle, précision ≤ 250 ms). */
export function useSlideTick(s: CastSeries): number {
  const { auto, interval_ms, since_ms } = s.slide;
  const [tick, setTick] = useState(() => slideTick(s, Date.now()));
  useEffect(() => {
    setTick(slideTick(s, Date.now()));
    if (!auto) return;
    const id = window.setInterval(() => setTick(slideTick(s, Date.now())), 250);
    return () => window.clearInterval(id);
    // `s` n'est lu que pour auto / interval / ancre : ces trois valeurs déclenchent le recalcul
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [auto, interval_ms, since_ms]);
  return tick;
}
