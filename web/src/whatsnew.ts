// Changelog produit (nos features storm-codex, distinct des patch notes HotS).
// `version` croissante (date) ; le panneau « What's new » montre les entrées plus récentes que la
// dernière vue (localStorage). Ajoute une entrée en tête à chaque lot de nouveautés.
export const APP_VERSION = "2026.06.16";

export const CHANGELOG: { version: string; title: string; items: string[] }[] = [
  {
    version: "2026.06.16",
    title: "Patch notes, synergies & timeline",
    items: [
      "Patch Notes intégrés (liste + détail) avec notification de nouveau patch.",
      "Fiche héros détaillée (par mode), fiche joueur enrichie, page Synergies.",
      "Timeline de match : kills / structures / objectifs + piste de pins.",
      "Filtres riches + recherche textuelle + filtres persistants sur Matches.",
      "Stats étendues, totaux d'équipe, courbe XP, awards par type (emoji).",
    ],
  },
];
