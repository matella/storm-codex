//! Routes du lobby live. `POST /api/lobby` reçoit les octets bruts du fichier de lobby (même
//! contrat que `/api/upload-raw` : `Bearer` + corps binaire), le décode, l'enrichit, le persiste et
//! diffuse `lobby.detected`. Le parseur vivant côté serveur, une casse de format Blizzard se
//! corrige par un redéploiement du box, sans jamais retoucher le binaire Windows.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value as J};

use crate::lobby::{store, LobbyState};
use crate::AppState;

/// Deuxième ligne de défense de l'idempotence, après la garde par hash de `ingest` (voir
/// [`decider`]) : celle-ci absorbe tout repost octet pour octet identique — y compris sur un
/// lobby déjà lié, ce qui protège le debrief d'un simple redémarrage du watcher qui repose sur
/// disque le même `replay.server.battlelobby`. `meme_lobby` ne voit donc jamais deux blobs
/// identiques ; elle ne traite que le cas où le hash a changé mais où les BattleTags pourraient
/// tout de même désigner « le même lobby » (le jeu réécrit le fichier en plusieurs passes autour
/// de la bonne lecture). On compare l'ensemble des BattleTags.
fn meme_lobby(a: &LobbyState, b: &LobbyState) -> bool {
    // (c) Un lobby déjà lié à un match (`a.match_id.is_some()`) décrit une partie terminée : un
    // blob dont le hash diffère (sinon la garde de `decider` l'aurait absorbé avant d'arriver
    // ici) mais dont les BattleTags coïncident n'est donc jamais « le même lobby » qu'un blob qui
    // vient d'être ingéré. Scénario réel : revanche en ligue/scrim entre la même composition (cf.
    // `FENETRE_LIAISON_HEURES` dans `lobby/mod.rs`, qui documente que ce cas est fréquent). Sans
    // cette garde, le blob de la partie 2 serait jugé identique à celui — déjà lié — de la partie
    // 1 : `unchanged`, pas de ré-enrichissement, pas de `lobby.detected`, et la page resterait
    // affichée en debrief de la partie 1 pendant tout le chargement de la partie 2.
    if a.match_id.is_some() {
        return false;
    }
    // (a) Idempotence des états illisibles. `unreadable()` produit `players: []` avec
    // `status: "parse_failed"` : sans ce cas, le garde `players.is_empty()` ci-dessous ferait
    // juger « différents » deux blobs illisibles reposés coup sur coup, donc chaque repost
    // resauvegarderait et rediffuserait `lobby.detected` pour rien. Deux `parse_failed` sans
    // joueur sont le même lobby.
    if a.status.as_deref() == Some("parse_failed") && b.status.as_deref() == Some("parse_failed") {
        return true;
    }
    if a.players.len() != b.players.len() || a.players.is_empty() {
        return false;
    }
    let mut x: Vec<&str> = a.players.iter().map(|p| p.battletag.as_str()).collect();
    let mut y: Vec<&str> = b.players.iter().map(|p| p.battletag.as_str()).collect();
    x.sort_unstable();
    y.sort_unstable();
    x == y
}

/// (b) Un `parse_failed` ne doit jamais écraser un lobby lisible tout juste détecté : le jeu
/// réécrit le fichier de lobby pendant quelques secondes autour de la bonne lecture, ce qui
/// produit une lecture tronquée transitoire — pas une vraie fin de partie. Deux parties réelles
/// sont séparées d'au moins une dizaine de minutes, donc une fenêtre de 2 minutes couvre
/// largement le cas transitoire sans jamais retarder un lobby réellement neuf, y compris le jour
/// où Blizzard casse le format et où tous les blobs deviennent illisibles (le lobby suivant, lui
/// aussi `parse_failed`, sera accepté dès que `prec` aura plus de 2 minutes).
fn parse_failed_transitoire(prec: &LobbyState, nouveau: &LobbyState) -> bool {
    if nouveau.status.as_deref() != Some("parse_failed") || prec.players.is_empty() {
        return false;
    }
    match chrono::DateTime::parse_from_rfc3339(&prec.detected_at) {
        Ok(detecte) => {
            chrono::Utc::now().signed_duration_since(detecte) < chrono::Duration::minutes(2)
        }
        // Date illisible : on n'accepte jamais de bloquer un état sur une date qu'on ne sait
        // pas interpréter — le nouvel état (parse_failed) passe normalement.
        Err(_) => false,
    }
}

/// Décision d'idempotence de `ingest`, extraite en fonction pure pour être testable sans
/// `AppState` (même motif que `meme_lobby`/`appliquer_teams`).
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Unchanged,
    Ingerer,
}

/// Règle appliquée dans cet ordre, `prec` étant l'état actuellement en mémoire (`None` si aucun
/// lobby n'a encore été détecté) :
/// 1. **hash identique à `prec`** → `Unchanged`, quoi qu'il arrive, y compris si `prec` est déjà
///    lié à un match. C'est la garde qui protège le debrief : `replay.server.battlelobby` survit
///    sur le disque entre deux parties, donc un redémarrage du watcher qui le repose ne doit
///    jamais réécraser un état déjà enrichi avec `match_id: None`.
/// 2. sinon, les règles existantes (`meme_lobby`, `parse_failed_transitoire`) s'appliquent
///    inchangées — la garde « déjà lié » de `meme_lobby` comprise : un blob différent avec les
///    mêmes BattleTags sur un lobby lié est une revanche, et doit être ingéré.
fn decider(prec: Option<&LobbyState>, nouveau: &LobbyState) -> Decision {
    let Some(prec) = prec else {
        return Decision::Ingerer;
    };
    if prec.content_hash == nouveau.content_hash {
        return Decision::Unchanged;
    }
    if meme_lobby(prec, nouveau) || parse_failed_transitoire(prec, nouveau) {
        return Decision::Unchanged;
    }
    Decision::Ingerer
}

/// `POST /api/lobby` — octets bruts, `Bearer` d'upload.
pub async fn ingest(
    State(s): State<AppState>,
    headers: HeaderMap,
    bytes: axum::body::Bytes,
) -> (StatusCode, Json<J>) {
    if !crate::upload::token_valide(&s.db, &headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "status": "unauthorized" })),
        );
    }

    // Calculé avant le décodage, sur les octets bruts : c'est le même helper que l'upload de
    // replay (`crate::upload::sha256_hex`), et il doit être posé même si le blob s'avère
    // illisible plus bas — un blob illisible reposté doit lui aussi être reconnu comme un repost.
    let content_hash = crate::upload::sha256_hex(&bytes);
    let detected_at = chrono::Utc::now().to_rfc3339();
    let mut state = match storm_lobby::parse(&bytes) {
        Ok(lobby) => LobbyState::from_lobby(&lobby, detected_at),
        Err(e) => {
            // Pas une erreur HTTP : la page doit s'ouvrir quand même, avec le sélecteur manuel.
            tracing::warn!("lobby illisible ({} octets) : {e}", bytes.len());
            LobbyState::unreadable(detected_at)
        }
    };
    state.content_hash = content_hash;

    {
        let courant = s.lobby.read().await;
        if decider(courant.as_ref(), &state) == Decision::Unchanged {
            return (StatusCode::OK, Json(json!({ "status": "unchanged" })));
        }
    }

    crate::lobby::enrich::enrich(&s.db, &mut state).await;
    if let Err(e) = store::save(&s.db, &state).await {
        tracing::error!("lobby save: {e}");
    }
    let joueurs = state.players.len();
    *s.lobby.write().await = Some(state);
    let _ = s.events.send(json!({ "type": "lobby.detected" }));

    (
        StatusCode::ACCEPTED,
        Json(json!({ "status": "ok", "players": joueurs })),
    )
}

/// `GET /api/lobby` — l'état enrichi courant, `204` si aucun lobby.
pub async fn get(State(s): State<AppState>) -> (StatusCode, Json<J>) {
    match s.lobby.read().await.as_ref() {
        None => (StatusCode::NO_CONTENT, Json(J::Null)),
        Some(st) => (
            StatusCode::OK,
            Json(serde_json::to_value(st).unwrap_or(J::Null)),
        ),
    }
}

/// `DELETE /api/lobby` — fermer le companion.
pub async fn clear(State(s): State<AppState>) -> Json<J> {
    if let Err(e) = store::clear(&s.db).await {
        tracing::error!("lobby clear: {e}");
    }
    *s.lobby.write().await = None;
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    Json(json!({ "ok": true }))
}

#[derive(serde::Deserialize)]
pub struct HeroBody {
    /// Clé `dim_heroes.id`. `null` pour effacer la saisie.
    pub hero: Option<String>,
}

/// `POST /api/lobby/hero` — le héros n'est jamais dans le blob : c'est le seul tap obligatoire.
pub async fn set_hero(State(s): State<AppState>, Json(b): Json<HeroBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| st.hero = b.hero).await
}

#[derive(serde::Deserialize)]
pub struct MapBody {
    pub map: Option<String>,
}

/// `POST /api/lobby/map` — repli quand la carte n'a pas pu être déduite des hashes `.s2ma`.
pub async fn set_map(State(s): State<AppState>, Json(b): Json<MapBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| {
        // `map_manual` reflète l'état réel : vrai seulement si une carte est effectivement
        // saisie, faux si la saisie est effacée (`map: null`) — jamais « corrigé » sur du vide.
        st.map_manual = b.map.is_some();
        st.map = b.map;
    })
    .await
}

#[derive(serde::Deserialize)]
pub struct TeamsBody {
    /// `battletag → équipe (0 ou 1)`. Les joueurs absents de la table gardent leur équipe.
    pub teams: std::collections::HashMap<String, u8>,
}

/// `POST /api/lobby/teams` — réassignation par joueur, pas un bouton d'inversion. Mesuré sur
/// 3 322 parties réelles (plan 1) : la déduction d'équipe est fiable à 100 % en matchmaking mais
/// se trompe souvent en partie personnalisée, et parmi les échecs seuls 5,3 % sont une inversion
/// des deux camps — les 94,7 % restants sont des ordres qui ne portent aucune information
/// d'équipe. Un bouton « inverser » serait donc inutile 19 fois sur 20 : seule une saisie
/// explicite par joueur peut reconstruire l'équipe correcte dans le cas général.
pub async fn set_teams(State(s): State<AppState>, Json(b): Json<TeamsBody>) -> (StatusCode, Json<J>) {
    muter(s, |st| appliquer_teams(&mut st.players, &b.teams)).await
}

/// Cœur de `set_teams`, extrait en fonction pure pour être testable sans `AppState` (pas de pool
/// Postgres, pas de `RwLock`). Strictement additive : un joueur absent de `teams`, ou dont la
/// valeur n'est pas 0/1, garde son équipe et son `team_manual` d'origine intacts.
fn appliquer_teams(
    players: &mut [crate::lobby::LobbyPlayerState],
    teams: &std::collections::HashMap<String, u8>,
) {
    for p in players {
        if let Some(t) = teams.get(&p.battletag) {
            if *t <= 1 {
                p.team = Some(*t);
                p.team_manual = true;
            }
        }
    }
}

/// Mutation + ré-enrichissement + persistance + diffusion, factorisés : les trois routes
/// `set_hero`/`set_map`/`set_teams` ci-dessus ne diffèrent que par la mutation elle-même.
///
/// Le write-lock est gardé pendant `enrich` (SQL) et `save` : c'est délibéré, pas un oubli, **pour
/// ces trois routes de saisie**. Le serveur n'a qu'un seul utilisateur donc la contention est
/// nulle — mais le verrou reste leur frontière de correction : deux saisies rapprochées depuis
/// l'UI ne doivent jamais s'entrelacer au milieu d'un enrichissement. `ingest` (le `POST
/// /api/lobby` qui reçoit le blob) est aussi une mutation mais suit un patron différent : il
/// n'enrichit qu'après avoir relâché le verrou de lecture qui sert à détecter les doublons — un
/// nouveau blob n'a pas besoin d'être empêché de s'entrelacer avec l'enrichissement d'un autre
/// puisqu'il n'y a par construction qu'un seul lobby en cours de détection à la fois côté jeu. La
/// diffusion WebSocket, elle, se fait après le `drop(guard)` explicite ci-dessous : elle n'a pas
/// besoin du verrou et ne doit pas retarder sa libération.
async fn muter<F>(s: AppState, f: F) -> (StatusCode, Json<J>)
where
    F: FnOnce(&mut LobbyState),
{
    let mut guard = s.lobby.write().await;
    let Some(st) = guard.as_mut() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "aucun lobby courant" })),
        );
    };
    f(st);
    crate::lobby::enrich::enrich(&s.db, st).await;
    if let Err(e) = store::save(&s.db, st).await {
        tracing::error!("lobby save: {e}");
    }
    let out = serde_json::to_value(&*st).unwrap_or(J::Null);
    drop(guard);
    let _ = s.events.send(json!({ "type": "lobby.updated" }));
    (StatusCode::OK, Json(out))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::lobby::LobbyPlayerState;

    fn joueur(battletag: &str) -> LobbyPlayerState {
        LobbyPlayerState {
            name: battletag.split('#').next().unwrap_or_default().to_string(),
            discriminant: String::new(),
            battletag: battletag.to_string(),
            team: None,
            team_manual: false,
            toon_handle: None,
            history: None,
        }
    }

    fn lobby_avec(battletags: &[&str]) -> LobbyState {
        LobbyState {
            schema_version: crate::lobby::LOBBY_SCHEMA_VERSION,
            detected_at: chrono::Utc::now().to_rfc3339(),
            players: battletags.iter().map(|b| joueur(b)).collect(),
            map: None,
            map_manual: false,
            hero: None,
            me: None,
            build: None,
            me_stats: None,
            match_id: None,
            status: None,
            content_hash: String::new(),
        }
    }

    fn dix_battletags() -> [&'static str; 10] {
        [
            "a#1", "b#2", "c#3", "d#4", "e#5", "f#6", "g#7", "h#8", "i#9", "j#10",
        ]
    }

    #[test]
    fn memes_battletags_ordre_different_sont_le_meme_lobby() {
        let mut mélange = dix_battletags();
        mélange.reverse();
        let a = lobby_avec(&dix_battletags());
        let b = lobby_avec(&mélange);
        assert!(meme_lobby(&a, &b));
    }

    #[test]
    fn un_battletag_different_donne_des_lobbys_differents() {
        let a = lobby_avec(&dix_battletags());
        let mut autre = dix_battletags();
        autre[9] = "quelqu_un_d_autre#99";
        let b = lobby_avec(&autre);
        assert!(!meme_lobby(&a, &b));
    }

    #[test]
    fn deux_parse_failed_sont_le_meme_lobby() {
        let a = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        let b = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(meme_lobby(&a, &b));
    }

    #[test]
    fn un_lobby_deja_lie_n_est_jamais_le_meme_qu_un_blob_entrant() {
        // Revanche entre les mêmes dix joueurs (ligue/scrim) : la partie 1 est déjà liée à son
        // replay (`match_id: Some(...)`) quand le lobby de la partie 2, même composition, arrive.
        // Sans la garde (c) de `meme_lobby`, ce test échouerait : les BattleTags identiques
        // feraient juger les deux lobbys identiques, et la partie 2 ne serait jamais ingérée.
        let mut a = lobby_avec(&dix_battletags());
        a.match_id = Some(42);
        let b = lobby_avec(&dix_battletags());
        assert!(!meme_lobby(&a, &b));
    }

    #[test]
    fn meme_hash_sur_lobby_deja_lie_est_unchanged() {
        // C'est le cas qui protège le debrief : `replay.server.battlelobby` survit sur le disque
        // entre deux parties, un redémarrage du watcher le repose à l'identique. Sans la garde
        // par hash en tête de `decider`, `meme_lobby` jugerait (à raison, vu la garde (c)) que ce
        // n'est *pas* le même lobby puisque `match_id.is_some()` — et écraserait le debrief.
        let mut prec = lobby_avec(&dix_battletags());
        prec.match_id = Some(42);
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "abc123".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Unchanged);
    }

    #[test]
    fn hash_different_memes_battletags_sur_lobby_lie_est_ingere() {
        // La revanche entre les mêmes dix joueurs doit continuer à être ingérée : ce test
        // vérifie que la garde par hash ne réintroduit pas le bug que la garde (c) de
        // `meme_lobby` avait corrigé.
        let mut prec = lobby_avec(&dix_battletags());
        prec.match_id = Some(42);
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "xyz789".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Ingerer);
    }

    #[test]
    fn meme_hash_sur_lobby_non_lie_est_unchanged() {
        // Repost ordinaire (watcher redémarré, lobby pas encore lié) : cas déjà couvert avant
        // l'introduction du hash, à ne pas régresser.
        let mut prec = lobby_avec(&dix_battletags());
        prec.content_hash = "abc123".to_string();
        let mut nouveau = lobby_avec(&dix_battletags());
        nouveau.content_hash = "abc123".to_string();
        assert_eq!(decider(Some(&prec), &nouveau), Decision::Unchanged);
    }

    #[test]
    fn aucun_lobby_courant_est_toujours_ingere() {
        let nouveau = lobby_avec(&dix_battletags());
        assert_eq!(decider(None, &nouveau), Decision::Ingerer);
    }

    #[test]
    fn lobby_lisible_et_parse_failed_sont_differents() {
        let a = lobby_avec(&dix_battletags());
        let b = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!meme_lobby(&a, &b));
        assert!(!meme_lobby(&b, &a));
    }

    #[test]
    fn parse_failed_juste_apres_un_lobby_lisible_est_transitoire() {
        let prec = lobby_avec(&dix_battletags());
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn parse_failed_apres_plus_de_deux_minutes_n_est_plus_transitoire() {
        let vieux = chrono::Utc::now() - chrono::Duration::minutes(3);
        let mut prec = lobby_avec(&dix_battletags());
        prec.detected_at = vieux.to_rfc3339();
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn parse_failed_sur_lobby_deja_illisible_n_est_pas_transitoire() {
        // Ce cas relève de `meme_lobby` (règle a), pas de la règle b : rien à ignorer ici.
        let prec = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn date_illisible_n_est_jamais_transitoire() {
        let mut prec = lobby_avec(&dix_battletags());
        prec.detected_at = "pas-une-date".to_string();
        let nouveau = LobbyState::unreadable(chrono::Utc::now().to_rfc3339());
        assert!(!parse_failed_transitoire(&prec, &nouveau));
    }

    #[test]
    fn appliquer_teams_reassigne_le_joueur_nomme_et_le_marque_manuel() {
        let mut joueurs = vec![joueur("a#1"), joueur("b#2")];
        let teams = std::collections::HashMap::from([("a#1".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, Some(1));
        assert!(joueurs[0].team_manual);
    }

    #[test]
    fn appliquer_teams_est_additive_les_absents_gardent_leur_etat() {
        // Propriété la plus importante : un joueur absent de la table ne doit être touché ni
        // dans son équipe ni dans son `team_manual`, même si un autre joueur du même appel est
        // réassigné. Ce test échouerait si `appliquer_teams` réinitialisait les joueurs absents
        // (par ex. `p.team = None` par défaut avant la boucle).
        let mut joueurs = vec![joueur("a#1"), joueur("b#2")];
        joueurs[1].team = Some(0);
        joueurs[1].team_manual = true;
        let teams = std::collections::HashMap::from([("a#1".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[1].team, Some(0));
        assert!(joueurs[1].team_manual);
    }

    #[test]
    fn appliquer_teams_ignore_une_valeur_hors_0_1() {
        // Ce test échouerait si le filtre `<= 1` était retiré : `team` passerait à `Some(2)` et
        // `team_manual` à `true` au lieu de rester intacts.
        let mut joueurs = vec![joueur("a#1")];
        let teams = std::collections::HashMap::from([("a#1".to_string(), 2u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, None);
        assert!(!joueurs[0].team_manual);
    }

    #[test]
    fn appliquer_teams_battletag_inconnu_n_a_aucun_effet_et_ne_panique_pas() {
        let mut joueurs = vec![joueur("a#1")];
        let teams = std::collections::HashMap::from([("inconnu#99".to_string(), 1u8)]);
        appliquer_teams(&mut joueurs, &teams);
        assert_eq!(joueurs[0].team, None);
        assert!(!joueurs[0].team_manual);
    }
}
