import { useId, type CSSProperties } from "react";

/** Champ de filtre avec recherche textuelle : input + `<datalist>` natif (typeahead). Tape pour
 *  filtrer une longue liste (héros, cartes…) au lieu de scroller un `<select>`. ✕ pour effacer. */
export function SearchSelect({
  options,
  value,
  onChange,
  placeholder,
  style,
}: {
  options: string[];
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  style?: CSSProperties;
}) {
  const id = useId();
  return (
    <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
      <input
        list={id}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        style={{ ...style, paddingRight: value ? 20 : (style as CSSProperties)?.paddingRight }}
      />
      {value && (
        <span
          onClick={() => onChange("")}
          title="clear"
          style={{ position: "absolute", right: 6, cursor: "pointer", color: "var(--text-2)", fontSize: 11, lineHeight: 1 }}
        >
          ✕
        </span>
      )}
      <datalist id={id}>
        {options.map((o) => <option key={o} value={o} />)}
      </datalist>
    </span>
  );
}
