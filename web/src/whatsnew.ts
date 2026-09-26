// Changelog produit (nos features storm-codex, distinct des patch notes HotS).
// `version` croissante (date) ; le panneau « What's new » montre les entrées plus récentes que la
// dernière vue (localStorage). Ajoute une entrée en tête à chaque lot de nouveautés.
export const APP_VERSION = "2026.09.26";

export const CHANGELOG: { version: string; title: string; items: string[] }[] = [
  {
    version: "2026.09.26",
    title: "Scouting reports",
    items: [
      "Nouvelle page Scouting : dépose les replays d'une équipe adverse, ses tendances sortent en chiffres (draft, cartes, joueurs, déroulé).",
      "Pack LLM à copier (Markdown) ou télécharger (.md / .xlsx) pour ChatGPT, Claude ou un modèle local.",
      "Import de l'analyse du LLM : chaque affirmation affiche les chiffres qui la fondent, les affirmations sans preuve sont signalées.",
      "Joueur ancre pour désigner l'équipe scoutée quand les replays ne suffisent pas (série contre un même adversaire).",
    ],
  },
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
