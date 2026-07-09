// Lecture animée (play/pause + vitesse) de la visionneuse Replay 2D. Le hook ne possède pas `t` —
// il pilote une boucle requestAnimationFrame et délègue l'avancement au composant via `onTick`.
import { useCallback, useEffect, useRef, useState } from "react";

/** Avance pure : next = t + dt*speed, clampée à [0, duration] ; auto-pause en fin de clip. */
export function advance(t: number, dt: number, speed: number, duration: number): { t: number; playing: boolean } {
  const next = t + dt * speed;
  if (next >= duration) return { t: duration, playing: false };
  return { t: next, playing: true };
}

export interface UsePlaybackOptions {
  onTick: (dtSeconds: number) => void;
}

export interface UsePlayback {
  playing: boolean;
  speed: number;
  toggle: () => void;
  setSpeed: (speed: number) => void;
  pause: () => void;
}

/** Boucle rAF déclenchant `onTick(dt)` tant que `playing` est vrai ; nettoyée au unmount/pause. */
export function usePlayback({ onTick }: UsePlaybackOptions): UsePlayback {
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const onTickRef = useRef(onTick);
  onTickRef.current = onTick;

  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    let last = performance.now();
    const loop = (now: number) => {
      const dt = (now - last) / 1000;
      last = now;
      onTickRef.current(dt);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [playing]);

  const toggle = useCallback(() => setPlaying((p) => !p), []);
  const pause = useCallback(() => setPlaying(false), []);

  return { playing, speed, toggle, setSpeed, pause };
}
