//! Benchmark runner.
//!
//! Connects to a VM/container and performs the following steps in order:
//!
//! 1. Install Rust (via rustup).
//! 2. Sparse-clone this repository to obtain the `workspace/` example.
//! 3. Run `cargo clippy --fix` on the workspace.
//! 4. Run `cargo build` (dev profile) on the workspace.
//! 5. Run `cargo build --release` on the workspace.
//!
//! Each step is timed and its host-side CPU / memory usage is tracked with
//! [`crate::metrics::Sampler`].

use anyhow::{bail, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::{
    metrics::{Sampler, StepMetrics},
    vm::Domain,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The name of a benchmark step (for display / serialisation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepName {
    InstallRust,
    CloneWorkspace,
    ClippyFix,
    BuildDev,
    BuildRelease,
}

impl std::fmt::Display for StepName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepName::InstallRust => write!(f, "Install Rust"),
            StepName::CloneWorkspace => write!(f, "Clone workspace"),
            StepName::ClippyFix => write!(f, "cargo clippy --fix"),
            StepName::BuildDev => write!(f, "cargo build (dev)"),
            StepName::BuildRelease => write!(f, "cargo build --release"),
        }
    }
}

/// Metrics + outcome for a single step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub name: StepName,
    pub metrics: StepMetrics,
    /// Exit code returned by the remote command (0 = success).
    pub exit_code: i32,
    /// Tail of stdout + stderr (last 4 KiB).
    pub output_tail: String,
}

impl StepResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

/// All step results for one VM run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub steps: Vec<StepResult>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const REPO_URL: &str = "https://github.com/tascord/icbm";
const WORKSPACE_PATH: &str = "icbm/workspace";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Connects to the VM/container via the domain's `exec` method and runs all
/// benchmark steps, returning their combined results.
pub async fn run(domain: &Domain) -> Result<BenchResult> {
    let mut steps = vec![];

    // -----------------------------------------------------------------------
    // Step 1: Install Rust
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            domain,
            StepName::InstallRust,
            // Use rustup in non-interactive mode; export PATH so subsequent
            // commands can find cargo without a new login shell.
            "curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable 2>&1 \
             && . $HOME/.cargo/env \
             && rustc --version",
        )
        .await?,
    );

    if !steps.last().unwrap().succeeded() {
        bail!("Rust installation failed – aborting benchmark");
    }

    // -----------------------------------------------------------------------
    // Step 2: Sparse-clone workspace
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            domain,
            StepName::CloneWorkspace,
            &format!(
                "git clone --filter=blob:none --sparse {REPO_URL} icbm 2>&1 \
                 && cd icbm \
                 && git sparse-checkout set workspace 2>&1"
            ),
        )
        .await?,
    );

    if !steps.last().unwrap().succeeded() {
        bail!("Workspace clone failed – aborting benchmark");
    }

    // -----------------------------------------------------------------------
    // Step 3: clippy --fix
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            domain,
            StepName::ClippyFix,
            &format!(
                ". $HOME/.cargo/env \
                 && cd {WORKSPACE_PATH} \
                 && cargo clippy --fix --allow-dirty --allow-staged 2>&1"
            ),
        )
        .await?,
    );

    // -----------------------------------------------------------------------
    // Step 4: cargo build (dev)
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            domain,
            StepName::BuildDev,
            &format!(
                ". $HOME/.cargo/env \
                 && cd {WORKSPACE_PATH} \
                 && cargo build 2>&1"
            ),
        )
        .await?,
    );

    // -----------------------------------------------------------------------
    // Step 5: cargo build --release
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            domain,
            StepName::BuildRelease,
            &format!(
                ". $HOME/.cargo/env \
                 && cd {WORKSPACE_PATH} \
                 && cargo build --release 2>&1"
            ),
        )
        .await?,
    );

    Ok(BenchResult { steps })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Measures a single remote command with a [`Sampler`] around it.
async fn run_step(domain: &Domain, name: StepName, cmd: &str) -> Result<StepResult> {
    println!("    {} {}…", "►".cyan(), name.to_string().bold());

    let sampler = Sampler::start();

    let (exit_code, output_tail) = domain.exec(cmd)?;

    let metrics = sampler.finish().await?;

    let status_icon = if exit_code == 0 {
        "✓".green().to_string()
    } else {
        "✗".red().to_string()
    };

    println!(
        "    {} {} — {:.1}s  avg CPU {:.1}%  peak mem {} MiB",
        status_icon,
        name.to_string().bold(),
        metrics.elapsed_secs,
        metrics.avg_cpu_pct,
        metrics.peak_memory_mib,
    );

    if exit_code != 0 {
        let tail_str = output_tail.trim();
        if !tail_str.is_empty() {
            println!("      {}", "--- Command Output ---".dimmed());
            for line in tail_str.lines() {
                println!("      {}", line.dimmed());
            }
            println!("      {}", "----------------------".dimmed());
        }
    }

    Ok(StepResult {
        name,
        metrics,
        exit_code,
        output_tail,
    })
}


