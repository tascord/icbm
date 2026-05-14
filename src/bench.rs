//! Benchmark runner.
//!
//! Connects to a VM over SSH and performs the following steps in order:
//!
//! 1. Install Rust (via rustup).
//! 2. Sparse-clone this repository to obtain the `workspace/` example.
//! 3. Run `cargo clippy --fix` on the workspace.
//! 4. Run `cargo build` (dev profile) on the workspace.
//! 5. Run `cargo build --release` on the workspace.
//!
//! Each step is timed and its host-side CPU / memory usage is tracked with
//! [`crate::metrics::Sampler`].

use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
    io::Read,
    net::TcpStream,
    path::Path,
    time::Duration,
};

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

/// Connects to `ip` via SSH (using the key stored by `domain`), then runs all
/// benchmark steps, returning their combined results.
pub async fn run(domain: &Domain, ip: &str) -> Result<BenchResult> {
    let sess = ssh_connect(ip, domain.ssh_port(), domain.ssh_user(), &domain.ssh_key_path()).await?;

    let mut steps = vec![];

    // -----------------------------------------------------------------------
    // Step 1: Install Rust
    // -----------------------------------------------------------------------
    steps.push(
        run_step(
            &sess,
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
            &sess,
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
            &sess,
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
            &sess,
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
            &sess,
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
async fn run_step(sess: &Session, name: StepName, cmd: &str) -> Result<StepResult> {
    println!("    {} {}…", "►".cyan(), name.to_string().bold());

    let sampler = Sampler::start();

    let (exit_code, output_tail) = exec_ssh(sess, cmd)?;

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

/// Opens an SSH session to `ip` authenticating with the given private key.
async fn ssh_connect(ip: &str, port: u16, user: &str, key: &Path) -> Result<Session> {
    // Retry for up to 60 s to handle race between VM boot and SSH daemon start.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);

    loop {
        let stream = TcpStream::connect(format!("{}:{}", ip, port));
        if let Ok(tcp) = stream {
            tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
            tcp.set_write_timeout(Some(Duration::from_secs(30)))?;

            let mut sess = Session::new().context("Failed to create SSH session")?;
            sess.set_tcp_stream(tcp);
            sess.handshake().context("SSH handshake failed")?;
            sess.userauth_pubkey_file(user, None, key, None)
                .context("SSH public-key auth failed")?;

            if sess.authenticated() {
                return Ok(sess);
            }
        }

        if std::time::Instant::now() > deadline {
            bail!("SSH connection to {}:{} timed out", ip, port);
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Runs `cmd` on the SSH session and returns `(exit_code, output_tail)`.
///
/// The SSH channel is created fresh for every command so steps are independent.
fn exec_ssh(sess: &Session, cmd: &str) -> Result<(i32, String)> {
    let mut channel = sess.channel_session().context("SSH channel open failed")?;
    channel.exec(cmd).context("SSH exec failed")?;

    let mut output = String::new();
    channel.read_to_string(&mut output)?;

    channel.wait_close()?;
    let exit_code = channel.exit_status()?;

    // Keep only the last 4 KiB to avoid huge allocations.
    let tail = if output.len() > 4096 {
        output[output.len() - 4096..].to_string()
    } else {
        output
    };

    Ok((exit_code, tail))
}
