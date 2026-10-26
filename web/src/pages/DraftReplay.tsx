import { DraftReplayPanel } from "../cast/DraftReplayPanel";

/** Enveloppe mince : la télécommande vit dans la régie (`/cast?scene=draft`, mode Replay) ; la route `/draft/replay` redirige. */
export function DraftReplay() {
  return <DraftReplayPanel />;
}
