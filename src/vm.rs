//! Docker-only VM provisioning.
//!
//! This module handles:
//! * Pulling a Docker image (Ubuntu 22.04 or NixOS).
//! * Starting a detached container.
//! * Installing base packages via `docker exec`.
//! * Tearing down the container.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::{fmt, process::Command, str::FromStr, time::Duration};
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Flavour
// ---------------------------------------------------------------------------

/// The OS flavour to benchmark.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavour {
    Ubuntu,
    NixOs,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Provider {
    Auto,
    Docker,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Auto => write!(f, "auto"),
            Provider::Docker => write!(f, "docker"),
        }
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Provider::Auto),
            "docker" => Ok(Provider::Docker),
            other => bail!("Unknown provider '{}'. Use: auto, docker", other),
        }
    }
}



impl fmt::Display for Flavour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Flavour::Ubuntu => write!(f, "ubuntu"),
            Flavour::NixOs => write!(f, "nixos"),
        }
    }
}

impl FromStr for Flavour {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ubuntu" => Ok(Flavour::Ubuntu),
            "nixos" => Ok(Flavour::NixOs),
            other => bail!("Unknown flavour '{}'. Use: ubuntu, nixos", other),
        }
    }
}

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

/// Represents a single Docker container for benchmarking.
pub struct Domain {
    pub flavour: Flavour,
    pub name: String,
}

impl Domain {
    pub fn new(flavour: Flavour) -> Self {
        let name = match flavour {
            Flavour::Ubuntu => "icbm-ubuntu".to_string(),
            Flavour::NixOs => "icbm-nixos".to_string(),
        };

        Domain { flavour, name }
    }

    // -----------------------------------------------------------------------
    // Provision
    // -----------------------------------------------------------------------

    /// Pulls the Docker image and starts a detached container, installing
    /// base packages so the benchmark steps don't have to.
    pub async fn provision(&self) -> Result<()> {
        println!("  {} Docker container '{}'", "Provisioning".bold(), self.name.cyan());

        check_tool("docker")?;

        let image = self.docker_image();
        run_cmd("docker", &["pull", &image])?;

        // Remove existing container (if any) to ensure a clean state
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();

        run_cmd(
            "docker",
            &["run", "-d", "--name", &self.name, &image, "sleep", "infinity"],
        )?;

        // Install base packages so the benchmark steps don't have to
        println!("  {} base packages inside container…", "Installing".dimmed());
        run_cmd(
            "docker",
            &[
                "exec",
                &self.name,
                "sh",
                "-c",
                "apt-get update -qq && apt-get install -y -qq curl git build-essential pkg-config libssl-dev unzip 2>&1",
            ],
        )?;

        println!("  {} container '{}' ready", "✓".green(), self.name.cyan());
        Ok(())
    }

    fn docker_image(&self) -> String {
        match self.flavour {
            Flavour::Ubuntu => "ubuntu:22.04".to_string(),
            Flavour::NixOs => "nixos/nix:latest".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Wait for readiness
    // -----------------------------------------------------------------------

    /// Polls `docker inspect` until the container reports "running".
    pub async fn wait_ready(&self) -> Result<()> {
        println!("  {} container '{}' …", "Waiting for".dimmed(), self.name);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            if std::time::Instant::now() > deadline {
                bail!("Timed out waiting for container '{}'", self.name);
            }
            let output = Command::new("docker")
                .args(["inspect", "-f", "{{.State.Status}}", &self.name])
                .output();
            if let Ok(o) = output {
                let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if status == "running" {
                    return Ok(());
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    }

    // -----------------------------------------------------------------------
    // Teardown
    // -----------------------------------------------------------------------

    /// Removes the container (best-effort).
    pub async fn destroy(&self) -> Result<()> {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
        println!("  {} container '{}' removed", "✓".green(), self.name.cyan());
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Exec
    // -----------------------------------------------------------------------

    /// Execute a command inside the container and return its exit code
    /// and combined stdout/stderr as a string.
    pub fn exec(&self, cmd: &str) -> Result<(i32, String)> {
        let output = Command::new("docker")
            .args(["exec", &self.name, "sh", "-c", cmd])
            .output()
            .with_context(|| format!("docker exec '{}' failed", self.name))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}{}", stdout, stderr);

        let exit_code = output.status.code().unwrap_or(-1);

        let tail = if combined.len() > 4096 {
            combined[combined.len() - 4096..].to_string()
        } else {
            combined
        };

        Ok((exit_code, tail))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn check_tool(name: &str) -> Result<()> {
    let found = Command::new("sh")
        .args(["-c", &format!("command -v {}", name)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        bail!(
            "Required tool '{}' not found. Install it before running icbm.",
            name
        );
    }
    Ok(())
}

fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("Failed to launch '{}'", program))?;

    if !status.success() {
        bail!("Command '{}' exited with status {}", program, status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_ubuntu() {
        let d = Domain::new(Flavour::Ubuntu);
        assert_eq!(d.name, "icbm-ubuntu");
    }

    #[test]
    fn test_domain_nixos() {
        let d = Domain::new(Flavour::NixOs);
        assert_eq!(d.name, "icbm-nixos");
    }
}
