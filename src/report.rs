//! Terminal and JSON report generation.

use colored::Colorize;

use crate::score::ScoredResult;

// ---------------------------------------------------------------------------
// Print per-flavour summary
// ---------------------------------------------------------------------------

pub fn print_summary(result: &ScoredResult) {
    println!(
        "\n  ┌─ {} Results ─────────────────────────────────",
        result.flavour.to_string().yellow().bold()
    );

    for s in &result.step_scores {
        let bar = score_bar(s.score);
        println!(
            "  │  {:30}  score={:4}  [{bar}]  time={:4}  cpu={:4}  mem={:4}",
            s.name,
            s.score,
            s.time_score,
            s.cpu_score,
            s.mem_score,
        );
    }

    println!("  ├─────────────────────────────────────────────────");
    println!(
        "  │  {:30}  {:4}",
        "TOTAL SCORE".bold(),
        result.total_score.to_string().bold().cyan()
    );
    println!("  │  {}", result.rating.bold());
    println!("  └─────────────────────────────────────────────────\n");
}

// ---------------------------------------------------------------------------
// Leaderboard (when multiple flavours were benchmarked)
// ---------------------------------------------------------------------------

pub fn print_leaderboard(results: &[ScoredResult]) {
    if results.len() < 2 {
        return;
    }

    println!(
        "\n{}",
        "  ═══════════════  LEADERBOARD  ═══════════════".bold().yellow()
    );

    let mut ranked: Vec<&ScoredResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.total_score.cmp(&a.total_score));

    for (i, r) in ranked.iter().enumerate() {
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            _ => "🥉",
        };
        println!(
            "  {}  {:8}  score={:4}  {}",
            medal,
            r.flavour.to_string().yellow(),
            r.total_score.to_string().cyan(),
            r.rating,
        );
    }
    println!();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn score_bar(score: u32) -> String {
    let filled = (score / 100).min(10) as usize;
    let empty = 10 - filled;
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    match score {
        700..=1000 => bar.green().to_string(),
        400..=699 => bar.yellow().to_string(),
        _ => bar.red().to_string(),
    }
}
