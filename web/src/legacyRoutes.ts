// Anciennes pages absorbées par la régie de cast : redirections (utilisées par App.tsx, testées).
export const LEGACY_REDIRECTS: readonly { path: string; to: string }[] = [
  { path: "draft", to: "/cast?scene=draft" },
  { path: "draft/replay", to: "/cast?scene=draft" },
];
