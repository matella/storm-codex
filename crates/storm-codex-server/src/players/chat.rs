//! Habitudes de chat d'un compte : **mesures seulement** (volume, destinataire, moment, mots
//! récurrents). Aucun jugement de toxicité, aucun texte exposé. Spec :
//! docs/specs/2026-11-08-signaux-replay-supplementaires-design.md (section C).

use serde::Serialize;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};

/// Parties minimales pour présenter des pourcentages.
pub const MIN_GAMES: i64 = 5;
/// Un mot n'est affiché qu'à partir de ce nombre d'occurrences (jamais un message isolé).
pub const MIN_WORD_COUNT: i64 = 3;
pub const MAX_WORDS: usize = 8;
const MIN_WORD_LEN: usize = 3;
/// Au-delà, ce n'est pas un mot (CJK sans espaces, phrase collée) : jamais affiché.
const MAX_WORD_LEN: usize = 20;

/// Mots vides ignorés (français et anglais).
const STOP_WORDS: &[&str] = &[
    "the", "and", "you", "for", "are", "not", "but", "with", "this", "that", "have", "was", "its",
    "your", "they", "them", "will", "can", "get", "got", "just", "out", "all", "one", "our", "has",
    "had", "his", "her", "who", "how", "now", "too", "yes", "les", "des", "est", "pas", "que",
    "qui", "une", "pour", "dans", "sur", "avec", "mais", "vous", "nous", "elle", "ils", "aux",
    "par", "mon", "ton", "son", "ces", "cet", "ses", "tes", "mes", "plus", "fait", "tout",
];

/// Un message de chat d'un compte, tel que lu dans `matches.data.messages`.
#[derive(Debug, Clone, Default)]
pub struct ChatRow {
    pub text: Option<String>,
    /// 0 = tous, 1 = équipe.
    pub recipient: Option<i64>,
    /// Secondes depuis le début de la partie.
    pub time: Option<f64>,
    /// Durée de la partie en secondes.
    pub length: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Timing {
    pub early: f64,
    pub mid: f64,
    pub late: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WordCount {
    pub word: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatHabits {
    pub games: i64,
    pub messages: i64,
    pub per_game: f64,
    pub all_share: f64,
    pub team_share: f64,
    pub timing: Timing,
    pub top_words: Vec<WordCount>,
    pub enough: bool,
}

fn share(n: i64, total: i64) -> f64 {
    if total == 0 {
        0.0
    } else {
        n as f64 / total as f64
    }
}

type RawRow = (Option<String>, Option<i64>, Option<f64>, Option<f64>);

/// Parties récentes prises en compte : borne le coût (p95 < 100 ms) et garde `games` et `messages`
/// sur la même fenêtre.
pub const WINDOW_GAMES: i64 = 500;

/// `(parties de la fenêtre, messages de chat du compte dans ces parties)` — les `WINDOW_GAMES`
/// parties les plus récentes du compte.
pub async fn load_rows(db: &PgPool, toon: &str) -> Result<(i64, Vec<ChatRow>), sqlx::Error> {
    load_rows_with(db, toon, WINDOW_GAMES).await
}

pub async fn load_rows_with(
    db: &PgPool,
    toon: &str,
    window: i64,
) -> Result<(i64, Vec<ChatRow>), sqlx::Error> {
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM match_players WHERE toon_handle = $1")
            .bind(toon)
            .fetch_one(db)
            .await?;
    let raw: Vec<RawRow> = sqlx::query_as(
        "WITH ids AS (
             SELECT m.id FROM match_players mp JOIN matches m ON m.id = mp.match_id
             WHERE mp.toon_handle = $1
             ORDER BY m.played_at DESC NULLS LAST, m.id DESC
             LIMIT $2)
         SELECT x ->> 'text', (x ->> 'recipient')::bigint, (x ->> 'time')::float8, m.length::float8
         FROM ids JOIN matches m ON m.id = ids.id
         CROSS JOIN LATERAL jsonb_array_elements(
             CASE WHEN jsonb_typeof(m.data -> 'messages') = 'array' THEN m.data -> 'messages' ELSE '[]'::jsonb END) x
         WHERE x ->> 'type' = '0' AND x ->> 'player' = $1",
    )
    .bind(toon)
    .bind(window)
    .fetch_all(db)
    .await?;
    let rows = raw
        .into_iter()
        .map(|(text, recipient, time, length)| ChatRow {
            text,
            recipient,
            time,
            length,
        })
        .collect();
    Ok((total.min(window), rows))
}

pub fn habits(games: i64, rows: &[ChatRow]) -> ChatHabits {
    let messages = rows.len() as i64;
    let (mut all, mut team, mut with_recipient) = (0i64, 0i64, 0i64);
    let (mut early, mut mid, mut late, mut dated) = (0i64, 0i64, 0i64, 0i64);
    let mut words: HashMap<String, i64> = HashMap::new();
    for r in rows {
        match r.recipient {
            Some(0) => {
                all += 1;
                with_recipient += 1;
            }
            Some(1) => {
                team += 1;
                with_recipient += 1;
            }
            Some(_) => with_recipient += 1,
            None => {}
        }
        if let (Some(t), Some(len)) = (r.time, r.length.filter(|l| *l > 0.0)) {
            let f = t / len;
            dated += 1;
            if f < 0.25 {
                early += 1;
            } else if f <= 0.75 {
                mid += 1;
            } else {
                late += 1;
            }
        }
        if let Some(text) = &r.text {
            // un mot compte une fois par message : ≥ 3 occurrences = ≥ 3 messages distincts
            let lower = text.to_lowercase();
            let distinct: HashSet<&str> = lower
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| (MIN_WORD_LEN..=MAX_WORD_LEN).contains(&w.chars().count()))
                .filter(|w| !w.chars().all(char::is_numeric))
                .filter(|w| !STOP_WORDS.contains(w))
                .collect();
            for w in distinct {
                *words.entry(w.to_owned()).or_default() += 1;
            }
        }
    }
    let mut top: Vec<WordCount> = words
        .into_iter()
        .filter(|(_, n)| *n >= MIN_WORD_COUNT)
        .map(|(word, count)| WordCount { word, count })
        .collect();
    top.sort_by(|a, b| b.count.cmp(&a.count).then(a.word.cmp(&b.word)));
    top.truncate(MAX_WORDS);
    ChatHabits {
        games,
        messages,
        per_game: if games > 0 {
            messages as f64 / games as f64
        } else {
            0.0
        },
        all_share: share(all, with_recipient),
        team_share: share(team, with_recipient),
        timing: Timing {
            early: share(early, dated),
            mid: share(mid, dated),
            late: share(late, dated),
        },
        top_words: top,
        enough: games >= MIN_GAMES,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn row(text: &str, recipient: i64, frac: f64) -> ChatRow {
        ChatRow {
            text: Some(text.into()),
            recipient: Some(recipient),
            time: Some(frac * 600.0),
            length: Some(600.0),
        }
    }

    #[test]
    fn aucun_message_donne_des_zeros_sans_nan() {
        let h = habits(10, &[]);
        assert_eq!(h.messages, 0);
        assert_eq!(h.per_game, 0.0);
        assert_eq!((h.all_share, h.team_share), (0.0, 0.0));
        assert_eq!(h.timing, Timing::default());
        assert!(h.top_words.is_empty());
        assert!(h.enough);
        let none = habits(0, &[]);
        assert_eq!(none.per_game, 0.0);
        assert!(!none.enough);
    }

    #[test]
    fn parts_destinataire_et_moment() {
        let rows = [
            row("a", 0, 0.1),
            row("b", 0, 0.5),
            row("c", 1, 0.5),
            row("d", 1, 0.9),
        ];
        let h = habits(2, &rows);
        assert_eq!(h.per_game, 2.0);
        assert_eq!((h.all_share, h.team_share), (0.5, 0.5));
        assert_eq!(
            h.timing,
            Timing {
                early: 0.25,
                mid: 0.5,
                late: 0.25
            }
        );
    }

    #[test]
    fn mot_rare_jamais_affiche() {
        let rows = [
            row("gg", 1, 0.9),
            row("gg", 1, 0.9),
            row("gg bonjour", 1, 0.9),
            row("bonjour", 1, 0.9),
        ];
        let h = habits(5, &rows);
        assert!(
            h.top_words.is_empty(),
            "« gg » fait 2 caractères ; « bonjour » 2 fois : sous le seuil"
        );
        let rows = [
            row("noob", 1, 0.5),
            row("noob", 1, 0.5),
            row("noob bonjour", 1, 0.5),
            row("bonjour", 1, 0.5),
        ];
        let h = habits(5, &rows);
        assert_eq!(
            h.top_words,
            vec![WordCount {
                word: "noob".into(),
                count: 3
            }]
        );
    }

    #[test]
    fn mots_non_latins_et_vides_ignores() {
        let rows = [
            row("Привет привет", 1, 0.5),
            row("ПРИВЕТ the the 123", 1, 0.5),
            row("привет 123 les les", 1, 0.5),
        ];
        let h = habits(5, &rows);
        assert_eq!(
            h.top_words,
            vec![WordCount {
                word: "привет".into(),
                count: 3
            }]
        );
    }

    #[test]
    fn un_seul_message_qui_repete_un_mot_n_affiche_rien() {
        let rows = [row("noob noob noob noob", 1, 0.5)];
        assert!(habits(5, &rows).top_words.is_empty());
    }

    #[test]
    fn phrase_sans_espaces_envoyee_trois_fois_n_est_pas_un_mot() {
        // CJK : aucune coupe sur les espaces, la phrase entière serait un seul « mot »
        let phrase = "这是一个很长很长很长很长很长很长很长的句子没有空格";
        let rows = [
            row(phrase, 1, 0.5),
            row(phrase, 1, 0.5),
            row(phrase, 1, 0.5),
        ];
        assert!(habits(5, &rows).top_words.is_empty());
    }

    #[test]
    fn message_sans_texte_ou_temps_compte_au_volume_seulement() {
        let rows = [
            ChatRow::default(),
            ChatRow {
                time: Some(10.0),
                length: Some(0.0),
                ..ChatRow::default()
            },
        ];
        let h = habits(5, &rows);
        assert_eq!(h.messages, 2);
        assert_eq!(h.timing, Timing::default());
        assert_eq!((h.all_share, h.team_share), (0.0, 0.0));
    }
}
