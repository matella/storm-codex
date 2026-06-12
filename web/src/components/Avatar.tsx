import { initials, universeColor } from "../api";

/** Avatar initiales à anneau d'univers (maquette Nexus Codex). */
export function Avatar({ hero, size = 24 }: { hero: string | null; size?: number }) {
  const ring = universeColor(hero);
  return (
    <span
      className="av"
      style={{
        width: size,
        height: size,
        border: `${Math.max(1.5, size / 16)}px solid ${ring}`,
        color: ring,
        background: "#181a22",
        fontSize: size * 0.34,
      }}
      title={hero ?? undefined}
    >
      {initials(hero)}
    </span>
  );
}
