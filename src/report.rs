//! Terminal and JSON report generation.

use {
    crate::{metrics, score::ScoredResult},
    colored::Colorize,
    std::fmt::Write,
};

// Sends to https://discord.gg/aKbXHBAcrR
const DISCORD_WEBHOOK_URL: &str = "https://discord.com/api/webhooks/1522111997716598865/SetBL5hAD3egA3zMQts0KEvuhVXWDTHxeFLsvgKZGPEk5zHN3X_F9qdBQogqHGPQ0enN";

// ---------------------------------------------------------------------------
// Print per-flavour summary
// ---------------------------------------------------------------------------

pub fn print_summary(result: &ScoredResult) {
    let terminal_report = render_summary(result, true);
    print!("{terminal_report}");

    let webhook_report = render_summary(result, false);
    send_webhook_report(&webhook_report);
}

// ---------------------------------------------------------------------------
// Leaderboard (when multiple flavours were benchmarked)
// ---------------------------------------------------------------------------

pub fn print_leaderboard(results: &[ScoredResult]) {
    if results.len() < 2 {
        return;
    }

    let terminal_report = render_leaderboard(results, true);
    print!("{terminal_report}");

    let webhook_report = render_leaderboard(results, false);
    send_webhook_report(&webhook_report);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn render_summary(result: &ScoredResult, color: bool) -> String {
    let mut out = String::new();

    writeln!(out, "\n  ┌─ {} Results ─────────────────────────────────", flavour_label(&result.flavour.to_string(), color))
        .expect("write to string");

    render_host_info(&mut out, color);

    for (s, step_result) in result.step_scores.iter().zip(result.bench.steps.iter()) {
        let bar = score_bar(s.score, color);
        let metrics = &step_result.metrics;

        if step_result.succeeded() {
            writeln!(
                out,
                "  │  {:30}  score={:4}  [{bar}]  time={:4.1}s  cpu={:3.0}%  mem={:4}M",
                s.name, s.score, metrics.elapsed_secs, metrics.avg_cpu_pct, metrics.peak_memory_mib,
            )
            .expect("write to string");
        } else {
            writeln!(
                out,
                "  │  {:30}  score={:4}  [{bar}]  {}                       ",
                s.name,
                s.score,
                failed_label(color),
            )
            .expect("write to string");
        }
    }

    writeln!(out, "  ├─────────────────────────────────────────────────").expect("write to string");
    writeln!(out, "  │  {:30}  {:4}", total_score_label(color), total_score_value(result.total_score, color))
        .expect("write to string");
    writeln!(out, "  │  {}", rating_label(&result.rating, color)).expect("write to string");
    writeln!(out, "  └─────────────────────────────────────────────────\n").expect("write to string");

    out
}

fn render_leaderboard(results: &[ScoredResult], color: bool) -> String {
    let mut out = String::new();

    writeln!(out).expect("write to string");
    writeln!(out, "{}", leaderboard_title(color)).expect("write to string");

    let mut ranked: Vec<&ScoredResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.total_score.cmp(&a.total_score));

    for (i, result) in ranked.iter().enumerate() {
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            _ => "🥉",
        };
        writeln!(
            out,
            "  {}  {:8}  score={:4}  {}",
            medal,
            flavour_label(&result.flavour.to_string(), color),
            score_value(result.total_score, color),
            result.rating,
        )
        .expect("write to string");
    }

    writeln!(out).expect("write to string");

    out
}

fn render_host_info(out: &mut String, color: bool) {
    let info = metrics::host_info();
    writeln!(out, "  │  {} {:24} {}  {}", host_label("OS", color), info.os, host_label("Host", color), info.hostname)
        .expect("write to string");
    writeln!(
        out,
        "  │  {} {:24} {}  {}",
        host_label("Kernel", color),
        info.kernel,
        host_label("Memory", color),
        format!("{} MiB", info.total_memory_mib)
    )
    .expect("write to string");
    writeln!(out, "  │  {}  {}", host_label("CPU", color), format!("{} ({} cores)", info.cpu_brand, info.cpu_count))
        .expect("write to string");
    writeln!(out, "  │").expect("write to string");
}

fn send_webhook_report(report: &str) {
    for chunk in split_report_chunks(report.trim_end(), 1800) {
        if let Err(error) = reqwest::blocking::Client::new()
            .post(DISCORD_WEBHOOK_URL)
            .json(&serde_json::json!({
                "content": null,
                "embeds": [
                    {
                        "description": format!("```\n{chunk}\n```"),
                        "color": 5814783
                    }
                ],
                "attachments": []
            }))
            .send()
            .and_then(|response| response.error_for_status())
        {
            eprintln!("failed to send benchmark report to Discord webhook: {error}");
            break;
        }
    }
}

fn split_report_chunks(report: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in report.lines() {
        let next_len = if current.is_empty() { line.len() } else { current.len() + 1 + line.len() };

        if next_len > max_len && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn score_bar(score: u32, color: bool) -> String {
    let filled = (score / 100).min(10) as usize;
    let empty = 10 - filled;
    let bar = "█".repeat(filled) + &"░".repeat(empty);
    if !color {
        return bar;
    }

    match score {
        700..=1000 => bar.green().to_string(),
        400..=699 => bar.yellow().to_string(),
        _ => bar.red().to_string(),
    }
}

fn flavour_label(flavour: &str, color: bool) -> String {
    if color { flavour.yellow().bold().to_string() } else { flavour.to_string() }
}

fn host_label(label: &str, color: bool) -> String { if color { label.cyan().bold().to_string() } else { label.to_string() } }

fn total_score_label(color: bool) -> String {
    if color { "TOTAL SCORE".bold().to_string() } else { "TOTAL SCORE".to_string() }
}

fn total_score_value(score: u32, color: bool) -> String {
    if color { score.to_string().bold().cyan().to_string() } else { score.to_string() }
}

fn rating_label(rating: &str, color: bool) -> String { if color { rating.bold().to_string() } else { rating.to_string() } }

fn failed_label(color: bool) -> String { if color { "FAILED".red().bold().to_string() } else { "FAILED".to_string() } }

fn leaderboard_title(color: bool) -> String {
    let title = "  ═══════════════  LEADERBOARD  ═══════════════";
    if color { title.bold().yellow().to_string() } else { title.to_string() }
}

fn score_value(score: u32, color: bool) -> String {
    if color { score.to_string().cyan().to_string() } else { score.to_string() }
}
