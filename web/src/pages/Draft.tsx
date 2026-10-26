import { DraftConsole } from "../cast/DraftWorkspace";

/** Enveloppe mince : la console vit dans la régie (`/cast?scene=draft`) ; la route `/draft` redirige. */
export function Draft() {
  return <DraftConsole />;
}
