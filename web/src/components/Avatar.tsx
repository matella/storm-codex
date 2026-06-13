import { useState } from "react";
import { initials, universeColor, heroIcon } from "../api";

/**
 * Avatar héros : portrait (vendorisé, /images/heroes/<slug>.png) cerclé de la couleur d'univers.
 * Fallback déterministe sur les initiales si le portrait est inconnu ou échoue à charger.
 */
export function Avatar({ hero, size = 24 }: { hero: string | null; size?: number }) {
  const ring = universeColor(hero);
  const icon = heroIcon(hero);
  const [broken, setBroken] = useState(false);
  const border = `${Math.max(1.5, size / 16)}px solid ${ring}`;

  if (icon && !broken) {
    return (
      <img
        src={icon}
        alt={hero ?? ""}
        title={hero ?? undefined}
        loading="lazy"
        onError={() => setBroken(true)}
        style={{
          width: size,
          height: size,
          borderRadius: "50%",
          objectFit: "cover",
          border,
          background: "#181a22",
          flexShrink: 0,
        }}
      />
    );
  }
  return (
    <span
      className="av"
      style={{ width: size, height: size, border, color: ring, background: "#181a22", fontSize: size * 0.34 }}
      title={hero ?? undefined}
    >
      {initials(hero)}
    </span>
  );
}
