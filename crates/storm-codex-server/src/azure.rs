//! Push post-game box→Azure (jalon 5) : POST sortant authentifié du résumé de partie vers
//! l'EBS Twitch Azure (même mécanisme que le patch-digest HotsPatchNotes). Optionnel
//! (`AZURE_PUSH_URL`/`AZURE_PUSH_TOKEN`). **Non testé contre la vraie EBS** : code prêt,
//! intégration à valider le soir avec le box. Best-effort : n'affecte jamais le parse.

use serde_json::Value as J;

/// POST `event` vers Azure en arrière-plan (spawn_blocking : ureq est synchrone).
pub fn push(url: String, token: Option<String>, event: J) {
    tokio::task::spawn_blocking(move || {
        let mut req = ureq::post(&url);
        if let Some(t) = &token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        match req.send_json(&event) {
            Ok(_) => tracing::debug!("push Azure ok"),
            Err(e) => tracing::warn!("push Azure échoué : {e}"),
        }
    });
}
