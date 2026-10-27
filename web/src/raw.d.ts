// Import du texte brut d'un fichier (`?raw`, Vite/Vitest) — utilisé par les tests qui vérifient des sources/CSS.
declare module "*?raw" {
  const text: string;
  export default text;
}
