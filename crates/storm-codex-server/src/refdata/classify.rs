//! Classification of a patch section's text: `BUFF` / `NERF` / `MIXED` / `REWORK` / `BUGFIX`, plus a
//! short human summary of the largest change ("Q damage 96 → 110 (+15%)"). Port of the old
//! HotsPatchNotes `PatchClassifier.cs` (commit `015c3776`, 81 lines of regex heuristics) — the dataset no
//! longer ships these verdicts (spec 2026-11-15, D2). Heuristic by design: numeric pairs ("from X to Y",
//! "X → Y") drive the verdict, with the direction INVERTED for cost/cooldown-like values (an increased
//! cooldown is a nerf). Pure, no I/O.

use regex::Regex;
use std::sync::OnceLock;

/// Verdict and summary of one section (`classification` is empty when there is no signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub classification: &'static str,
    pub short_summary: String,
}

struct Patterns {
    pair: Regex,
    rework: Regex,
    inverted: Regex,
    bugfix: Regex,
    spaces: Regex,
    label_prefix: Regex,
}

/// Compiled once. The patterns are literals, so a failure is a programming error caught by the tests.
fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |s: &str| Regex::new(s).unwrap_or_else(|e| panic!("classifier regex {s}: {e}"));
        Patterns {
            pair: re(
                r"(?i)(?<ctx>[^.;\n]{0,70}?)(?:from\s+)?(?<a>\d+(?:\.\d+)?)\s*(?:→|to)\s*(?<b>\d+(?:\.\d+)?)",
            ),
            rework: re(r"(?i)\b(rework|redesign|overhaul)"),
            inverted: re(r"(?i)\b(cooldown|cost|mana|recharge|cast time|delay)\b"),
            bugfix: re(r"(?i)\b(fixed|no longer|corrected)\b"),
            spaces: re(r"\s+"),
            label_prefix: re(r"(?i)^(and|the|now|increased|reduced|lowered|raised)\s+"),
        }
    })
}

/// Classifies the plain text of a section (the dataset's `sections[].text`).
pub fn classify(text: &str) -> Verdict {
    let p = patterns();
    if p.rework.is_match(text) {
        return Verdict {
            classification: "REWORK",
            short_summary: "rework".into(),
        };
    }
    let (mut buffs, mut nerfs) = (0u32, 0u32);
    let mut best_pct = 0.0f64;
    let mut best_summary = String::new();
    for m in p.pair.captures_iter(text) {
        let (Some(ctx), Some(a_s), Some(b_s)) = (m.name("ctx"), m.name("a"), m.name("b")) else {
            continue;
        };
        let (Ok(a), Ok(b)) = (a_s.as_str().parse::<f64>(), b_s.as_str().parse::<f64>()) else {
            continue;
        };
        if a == 0.0 || a == b {
            continue;
        }
        let up = b > a;
        let inverted = p.inverted.is_match(ctx.as_str());
        if inverted != up {
            buffs += 1;
        } else {
            nerfs += 1;
        }
        let pct = ((b - a) / a).abs() * 100.0;
        if pct > best_pct {
            let label = clean_label(ctx.as_str());
            let sign = if up { "+" } else { "−" };
            let rounded = round_half_even(pct, if pct < 10.0 { 1 } else { 0 });
            best_summary = format!(
                "{label}{} → {} ({sign}{rounded}%)",
                a_s.as_str(),
                b_s.as_str()
            );
            best_pct = pct;
        }
    }
    let classification = match (buffs, nerfs) {
        (1.., 0) => "BUFF",
        (0, 1..) => "NERF",
        (1.., 1..) => "MIXED",
        _ if p.bugfix.is_match(text) => "BUGFIX",
        _ => "",
    };
    Verdict {
        classification,
        short_summary: best_summary,
    }
}

/// Keeps the tail of the context (the words right before the numbers) and tidies it up.
fn clean_label(ctx: &str) -> String {
    let p = patterns();
    let label = p.spaces.replace_all(ctx, " ");
    let label = p.label_prefix.replace(label.trim(), "").into_owned();
    let chars: Vec<char> = label.chars().collect();
    let label = if chars.len() > 34 {
        format!("…{}", chars[chars.len() - 33..].iter().collect::<String>())
    } else {
        label
    };
    if label.is_empty() {
        String::new()
    } else {
        format!("{} ", label.trim_end_matches([':', ' ']))
    }
}

/// `Math.Round(v, digits)` of .NET: rounds half to even (banker's rounding), printed without a
/// trailing `.0` like `double.ToString(InvariantCulture)`.
fn round_half_even(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    let x = v * f;
    let r = x.round();
    let r = if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    };
    r / f
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    //! The old service's `PatchClassifierTests.cs`, ported verbatim (spec acceptance criterion 6).
    use super::*;

    #[test]
    fn increased_damage_is_buff_with_summary() {
        let r = classify("Thunder Clap damage increased from 96 to 110.");
        assert_eq!(r.classification, "BUFF");
        assert!(r.short_summary.contains("96 → 110"), "{}", r.short_summary);
        assert!(r.short_summary.contains("+15"), "{}", r.short_summary);
    }

    #[test]
    fn reduced_health_is_nerf() {
        let r = classify("Base maximum Health reduced from 2890 to 2760.");
        assert_eq!(r.classification, "NERF");
        assert!(r.short_summary.contains("−4.5%"), "{}", r.short_summary);
    }

    #[test]
    fn increased_cooldown_is_nerf_direction_inverted() {
        assert_eq!(
            classify("Cooldown increased from 8 to 10 seconds.").classification,
            "NERF"
        );
    }

    #[test]
    fn reduced_mana_cost_is_buff_direction_inverted() {
        assert_eq!(
            classify("Mana cost reduced from 40 to 35.").classification,
            "BUFF"
        );
    }

    #[test]
    fn buff_and_nerf_is_mixed() {
        assert_eq!(
            classify("Damage increased from 100 to 120. Health reduced from 2000 to 1800.")
                .classification,
            "MIXED"
        );
    }

    #[test]
    fn rework_keyword_wins_over_numbers() {
        assert_eq!(
            classify("Talents reworked. Damage increased from 10 to 20.").classification,
            "REWORK"
        );
    }

    #[test]
    fn fixed_only_is_bugfix() {
        let r = classify("Fixed an issue where Avatar no longer interrupted channels.");
        assert_eq!(r.classification, "BUGFIX");
        assert_eq!(r.short_summary, "");
    }

    #[test]
    fn no_signal_is_empty() {
        assert_eq!(
            classify("Developer comment: we like where this hero is.").classification,
            ""
        );
    }

    #[test]
    fn arrow_notation_is_parsed() {
        let r = classify("Slow duration 2 → 2.5 seconds.");
        assert_eq!(r.classification, "BUFF");
        assert!(r.short_summary.contains("2 → 2.5"), "{}", r.short_summary);
    }

    /// Not in the old suite: the summary seen on the old service for a real section
    /// (patch-notes-november-16-2023, "General"), to pin the label trimming and the rounding.
    #[test]
    fn summary_matches_the_old_service_on_a_real_section() {
        let r = classify(
            "Base experience value of Experience Globes increased from 80 to 160. \
             Experience value of Melee Minions reduced from 84 to 0.",
        );
        assert_eq!(
            r.short_summary,
            "…ue of Experience Globes increased 80 → 160 (+100%)"
        );
    }

    #[test]
    fn rounding_is_half_to_even_like_dotnet() {
        assert_eq!(round_half_even(2.5, 0), 2.0);
        assert_eq!(round_half_even(3.5, 0), 4.0);
        assert_eq!(round_half_even(4.4978, 1), 4.5);
    }
}
