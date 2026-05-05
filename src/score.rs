//! Scoring algorithm.
//!
//! The score is a single integer in [0, 1000] where **higher is better**.
//!
//! It is computed by combining normalised sub-scores for each benchmark step:
//!
//! | Dimension           | Weight |
//! |---------------------|--------|
//! | Wall-clock time     | 40 %   |
//! | Average CPU usage   | 30 %   |
//! | Peak memory usage   | 30 %   |
//!
//! For time and memory lower is better, so the raw value is inverted before
//! normalisation.  CPU utilisation at 100 % is not inherently bad (it means
//! the machine is using all its cores), so this dimension rewards higher
//! *sustained* utilisation divided by the elapsed time – i.e. doing the same
//! work faster while keeping CPUs busy scores best.
//!
//! The reference ceilings used for normalisation are generous upper bounds
//! observed on very slow machines:
//!
//! | Dimension           | Reference ceiling              |
//! |---------------------|-------------------------------|
//! | Time per step       | 600 s (10 min)                |
//! | Peak memory         | 8 192 MiB                     |
//!
//! These are intentionally high so that typical machines score in the
//! 400–800 range and truly fast machines can approach 1000.

use serde::{Deserialize, Serialize};

use crate::{
    bench::BenchResult,
    vm::Flavour,
};

// ---------------------------------------------------------------------------
// Reference ceilings
// ---------------------------------------------------------------------------

const REF_TIME_SECS: f64 = 600.0;
const REF_MEM_MIB: f64 = 8192.0;

// ---------------------------------------------------------------------------
// Weights
// ---------------------------------------------------------------------------

const W_TIME: f64 = 0.40;
const W_CPU: f64 = 0.30;
const W_MEM: f64 = 0.30;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Per-step score breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepScore {
    pub name: String,
    /// 0–1000
    pub score: u32,
    /// Time sub-score (0–1000)
    pub time_score: u32,
    /// CPU sub-score (0–1000)
    pub cpu_score: u32,
    /// Memory sub-score (0–1000)
    pub mem_score: u32,
}

/// Complete scored result for one VM run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredResult {
    pub flavour: Flavour,
    /// Overall score (0–1000, higher = better machine for Rust dev)
    pub total_score: u32,
    /// Qualitative label derived from `total_score`.
    pub rating: String,
    pub step_scores: Vec<StepScore>,
    pub bench: BenchResult,
}

// ---------------------------------------------------------------------------
// evaluate
// ---------------------------------------------------------------------------

pub fn evaluate(flavour: Flavour, bench: BenchResult) -> ScoredResult {
    let mut step_scores = vec![];

    for step in &bench.steps {
        if !step.succeeded() {
            step_scores.push(StepScore {
                name: step.name.to_string(),
                score: 0,
                time_score: 0,
                cpu_score: 0,
                mem_score: 0,
            });
            continue;
        }

        let m = &step.metrics;

        // Time: lower elapsed = higher score.
        // score = (1 - elapsed / ref_time).clamp(0, 1) * 1000
        let time_frac = (m.elapsed_secs / REF_TIME_SECS).min(1.0);
        let time_score = ((1.0 - time_frac) * 1000.0).round() as u32;

        // CPU: reward high average utilisation (machine is keeping cores busy)
        // scaled by speed (lower elapsed = better).  Formula:
        //   cpu_score = avg_cpu_pct/100 * (1 - time_frac) * 1000
        let cpu_score =
            ((m.avg_cpu_pct / 100.0) * (1.0 - time_frac) * 1000.0).round() as u32;

        // Memory: lower peak = higher score.
        let mem_frac = (m.peak_memory_mib as f64 / REF_MEM_MIB).min(1.0);
        let mem_score = ((1.0 - mem_frac) * 1000.0).round() as u32;

        let score = ((time_score as f64 * W_TIME)
            + (cpu_score as f64 * W_CPU)
            + (mem_score as f64 * W_MEM))
            .round() as u32;

        step_scores.push(StepScore {
            name: step.name.to_string(),
            score,
            time_score,
            cpu_score,
            mem_score,
        });
    }

    // Total = average of per-step scores (successful steps only).
    let valid: Vec<u32> = step_scores
        .iter()
        .filter(|s| s.score > 0)
        .map(|s| s.score)
        .collect();

    let total_score = if valid.is_empty() {
        0
    } else {
        (valid.iter().sum::<u32>() as f64 / valid.len() as f64).round() as u32
    };

    let rating = rating_label(total_score);

    ScoredResult {
        flavour,
        total_score,
        rating,
        step_scores,
        bench,
    }
}

fn rating_label(score: u32) -> String {
    match score {
        900..=1000 => "⚡ Exceptional – top-tier Rust development machine".to_string(),
        750..=899 => "🚀 Excellent – very well suited for Rust development".to_string(),
        550..=749 => "✅ Good – comfortable for most Rust projects".to_string(),
        350..=549 => "⚠️  Adequate – usable but expect slow release builds".to_string(),
        _ => "❌ Poor – not recommended for serious Rust development".to_string(),
    }
}


