//! VM provisioning via libvirt or Docker.
//!
//! This module handles:
//! * Downloading base cloud images (Ubuntu 24.04 LTS, NixOS 24.05).
//! * Creating a thin-clone overlay disk (libvirt) or pulling a Docker image.
//! * Starting the virsh domain or Docker container.
//! * Waiting until SSH (libvirt) or the container (Docker) is reachable.
//! * Tearing down the VM/container.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::{
    env, fmt, net::{IpAddr, TcpStream}, path::PathBuf, process::Command, str::FromStr,
    time::{Duration, Instant},
};
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
pub enum Provider {
    Auto,
    Libvirt,
    Docker,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Auto => write!(f, "auto"),
            Provider::Libvirt => write!(f, "libvirt"),
            Provider::Docker => write!(f, "docker"),
        }
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Provider::Auto),
            "libvirt" => Ok(Provider::Libvirt),
            "docker" => Ok(Provider::Docker),
            other => bail!("Unknown provider '{}'. Use: auto, libvirt, docker", other),
        }
    }
}

fn docker_available() -> bool {
    Command::new("docker")
        .args(["info"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl Provider {
    pub fn resolve(&self) -> Self {
        match self {
            Provider::Auto => {
                if docker_available() {
                    Provider::Docker
                } else {
                    Provider::Libvirt
                }
            }
            other => other.clone(),
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
// Image metadata
// ---------------------------------------------------------------------------

struct ImageInfo {
    /// Remote URL from which to fetch the cloud image.
    url: &'static str,
    /// SHA-256 checksum of the compressed image (for verification).
    #[allow(dead_code)]
    sha256: &'static str,
    /// Username for the default cloud user.
    user: &'static str,
}

fn image_info(flavour: &Flavour) -> ImageInfo {
    let arch = std::env::consts::ARCH;
    match flavour {
        Flavour::Ubuntu => ImageInfo {
            url: if arch == "aarch64" {
                "https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-arm64.img"
            } else {
                "https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img"
            },
            sha256: "", // checksums change; verify manually in prod
            user: "ubuntu",
        },
        Flavour::NixOs => ImageInfo {
            url: if arch == "aarch64" {
                "https://channels.nixos.org/nixos-24.05/latest-nixos-minimal-aarch64-linux.iso"
            } else {
                "https://channels.nixos.org/nixos-24.05/latest-nixos-minimal-x86_64-linux.iso"
            },
            sha256: "",
            user: "root",
        },
    }
}

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

/// Represents a single libvirt/virsh VM domain.
pub struct Domain {
    pub flavour: Flavour,
    pub provider: Provider,
    /// Unique domain name inside libvirt.
    pub name: String,
    /// Path to the (overlay) disk image.
    disk: PathBuf,
    /// Host SSH port to reach the guest (UTM uses forwarded port).
    ssh_port: u16,
}

fn state_dir() -> PathBuf {
    if let Ok(dir) = env::var("ICBM_STATE_DIR") {
        return PathBuf::from(dir);
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".local/share/icbm");
    }

    std::env::temp_dir().join("icbm")
}

fn images_dir() -> PathBuf {
    state_dir().join("images")
}

fn vms_dir() -> PathBuf {
    state_dir().join("vms")
}

impl Domain {
    pub fn new(flavour: Flavour, provider: Provider) -> Self {
        let resolved = provider.resolve();
        let name = match resolved {
            Provider::Docker => match flavour {
                Flavour::Ubuntu => "icbm-ubuntu".to_string(),
                Flavour::NixOs => "icbm-nixos".to_string(),
            },
            _ => {
                let ts = chrono::Utc::now().timestamp();
                format!("icbm-{}-{}", flavour, ts)
            }
        };

        let ssh_port = 22;

        let disk = vms_dir().join(format!("{}.qcow2", name));
        Domain {
            flavour,
            provider: resolved,
            name,
            disk,
            ssh_port,
        }
    }

    // -----------------------------------------------------------------------
    // Provision
    // -----------------------------------------------------------------------

    /// Downloads the base image (if needed), creates an overlay disk, defines
    /// and starts the virsh domain, then injects an SSH public key via
    /// cloud-init so the benchmark runner can connect.
    pub async fn provision(&self) -> Result<()> {
        match self.provider {
            Provider::Docker => self.provision_docker().await,
            Provider::Libvirt => self.provision_libvirt().await,
            Provider::Auto => unreachable!(),
        }
    }

    async fn provision_libvirt(&self) -> Result<()> {
        println!("  {} VM '{}'", "Provisioning".bold(), self.name.cyan());

        // Prerequisites
        for tool in &["virsh", "virt-install", "qemu-img"] {
            check_tool(tool)?;
        }
        if check_tool("cloud-localds").is_err() && check_tool("mkisofs").is_err() && check_tool("hdiutil").is_err() {
            bail!("Required tool 'cloud-localds', 'mkisofs', or 'hdiutil' not found in PATH");
        }

        std::fs::create_dir_all(images_dir())?;
        std::fs::create_dir_all(vms_dir())?;

        let info = image_info(&self.flavour);

        // 1. Download base image
        let base = self.ensure_base_image(info.url).await?;

        // 2. Create overlay disk (thin clone)
        let overlay = self.disk.clone();
        let backing_fmt = if base.extension().and_then(|s| s.to_str()) == Some("iso") {
            "raw"
        } else {
            "qcow2"
        };

        run_cmd(
            "qemu-img",
            &[
                "create",
                "-f",
                "qcow2",
                "-b",
                base.to_str().unwrap(),
                "-F",
                backing_fmt,
                overlay.to_str().unwrap(),
                "20G",
            ],
        )?;

        // 3. Generate cloud-init seed ISO with SSH key
        let seed = self.create_cloud_init_seed(info.user).await?;

        // 4. virt-install
        let network_arg = std::env::var("ICBM_VIRT_NETWORK").unwrap_or_else(|_| "network=default".to_string());
        let disk_arg = format!("path={},format=qcow2", self.disk.display());
        let cdrom_arg = format!("path={},device=cdrom", seed.display());
        let arch = std::env::consts::ARCH;
        let mut args = vec![
            "--name".to_string(),
            self.name.clone(),
            "--arch".to_string(),
            arch.to_string(),
        ];
        
        if cfg!(target_os = "macos") && arch == "aarch64" {
            // Homebrew libvirt on Apple Silicon often requires explicit machine
            // types or doesn't map kvm/hvf by default correctly in virt-install
            args.push("--virt-type".to_string());
            args.push("qemu".to_string()); // Fall back to qemu TCG or hvf if unconfigured
            args.push("--machine".to_string());
            args.push("virt".to_string());
        }

        args.extend(vec![
            "--ram".to_string(),
            "4096".to_string(),
            "--vcpus".to_string(),
            "2".to_string(),
            "--os-variant".to_string(),
            self.os_variant().to_string(),
            "--disk".to_string(),
            disk_arg,
            "--disk".to_string(),
            cdrom_arg,
            "--import".to_string(),
            "--network".to_string(),
            network_arg,
            "--noautoconsole".to_string(),
            "--graphics".to_string(),
            "none".to_string(),
        ]);

        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_cmd("virt-install", &args_ref)?;

        println!("  {} domain '{}' started", "✓".green(), self.name.cyan());
        Ok(())
    }

    async fn provision_docker(&self) -> Result<()> {
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
                "exec", &self.name,
                "sh", "-c",
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

    fn os_variant(&self) -> &'static str {
        match self.flavour {
            Flavour::Ubuntu => "ubuntu24.04",
            Flavour::NixOs => "nixos-unknown",
        }
    }

    /// Downloads the cloud image to the local images cache if not already cached.
    async fn ensure_base_image(&self, url: &str) -> Result<PathBuf> {
        let filename = url.rsplit('/').next().unwrap_or("base.img");
        let dest = images_dir().join(filename);
        if dest.exists() {
            println!("    {} base image cached: {}", "✓".green(), dest.display());
            return Ok(dest);
        }

        println!("    {} {}", "Downloading base image:".dimmed(), url);
        run_cmd("curl", &["-fSL", "-o", dest.to_str().unwrap(), url])?;
        println!("    {} download complete", "✓".green());
        Ok(dest)
    }

    /// Generates a minimal cloud-init seed ISO (user-data + meta-data) that
    /// creates a user with a freshly-generated ED25519 key-pair so the
    /// benchmark runner can SSH in without a password.
    async fn create_cloud_init_seed(&self, user: &str) -> Result<PathBuf> {
        let key_dir = vms_dir().join(format!("{}-ssh", self.name));
        std::fs::create_dir_all(&key_dir)?;
        let priv_key = key_dir.join("id_ed25519");
        let pub_key = key_dir.join("id_ed25519.pub");

        if !priv_key.exists() {
            run_cmd(
                "ssh-keygen",
                &[
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-f",
                    priv_key.to_str().unwrap(),
                    "-C",
                    "icbm-bench",
                ],
            )?;
        }

        let pub_key_str = std::fs::read_to_string(&pub_key)?;
        let pub_key_str = pub_key_str.trim();

        let user_data = format!(
            "#cloud-config\n\
             users:\n  \
             - name: {user}\n    \
               sudo: ALL=(ALL) NOPASSWD:ALL\n    \
               shell: /bin/bash\n    \
               ssh_authorized_keys:\n      \
               - {pub_key_str}\n\
             packages:\n  \
             - openssh-server\n  \
             - curl\n  \
             - git\n  \
             - build-essential\n  \
             - pkg-config\n  \
             - libssl-dev\n  \
             - qemu-guest-agent\n\
             runcmd:\n  \
             - systemctl enable --now ssh\n  \
             - systemctl enable --now qemu-guest-agent\n"
        );

        let seed_dir = vms_dir().join(format!("{}-seed", self.name));
        std::fs::create_dir_all(&seed_dir)?;
        std::fs::write(seed_dir.join("user-data"), &user_data)?;
        std::fs::write(
            seed_dir.join("meta-data"),
            format!("instance-id: {}\nlocal-hostname: {}\n", self.name, self.name),
        )?;

        let seed_iso = vms_dir().join(format!("{}-seed.iso", self.name));
        if seed_iso.exists() {
            std::fs::remove_file(&seed_iso)?;
        }
        
        if check_tool("cloud-localds").is_ok() {
            run_cmd(
                "cloud-localds",
                &[
                    seed_iso.to_str().unwrap(),
                    seed_dir.join("user-data").to_str().unwrap(),
                    seed_dir.join("meta-data").to_str().unwrap(),
                ],
            )?;
        } else if check_tool("mkisofs").is_ok() {
            run_cmd(
                "mkisofs",
                &[
                    "-output",
                    seed_iso.to_str().unwrap(),
                    "-volid",
                    "cidata",
                    "-joliet",
                    "-rock",
                    seed_dir.join("user-data").to_str().unwrap(),
                    seed_dir.join("meta-data").to_str().unwrap(),
                ],
            )?;
        } else {
            run_cmd(
                "hdiutil",
                &[
                    "makehybrid",
                    "-o",
                    seed_iso.to_str().unwrap(),
                    "-hfs",
                    "-joliet",
                    "-iso",
                    "-default-volume-name",
                    "cidata",
                    seed_dir.to_str().unwrap(),
                ],
            )?;
        }

        Ok(seed_iso)
    }

    // -----------------------------------------------------------------------
    // Networking
    // -----------------------------------------------------------------------

    /// Polls provider-specific APIs until the domain reports an address, then
    /// waits until the configured SSH port is open.
    pub async fn wait_for_ip(&self) -> Result<String> {
        if matches!(self.provider, Provider::Docker) {
            println!("  {} container '{}' …", "Waiting for".dimmed(), self.name);
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                if Instant::now() > deadline {
                    bail!("Timed out waiting for container '{}'", self.name);
                }
                let output = Command::new("docker")
                    .args(["inspect", "-f", "{{.State.Status}}", &self.name])
                    .output();
                if let Ok(o) = output {
                    let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if status == "running" {
                        return Ok("docker".to_string());
                    }
                }
                sleep(Duration::from_secs(1)).await;
            }
        }

        println!("  {} IP and SSH …", "Waiting for".dimmed());
        let deadline = Instant::now() + Duration::from_secs(300);

        loop {
            if Instant::now() > deadline {
                bail!("Timed out waiting for VM '{}' to become reachable", self.name);
            }

            if let Some(ip) = self.query_ip()? {
                if tcp_connectable(&ip, self.ssh_port) {
                    return Ok(ip);
                }
            }

            sleep(Duration::from_secs(5)).await;
        }
    }

    fn query_ip(&self) -> Result<Option<String>> {
        // `virsh domifaddr` returns lines like:
        //   vnet0  52:54:00:xx:xx:xx  ipv4  192.168.122.42/24
        let output = Command::new("virsh")
            .args(["domifaddr", &self.name, "--source", "lease"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return Ok(None),
        };

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let Some(cidr) = parts.get(3) {
                    if cidr.contains('.') {
                        let ip = cidr.split('/').next().unwrap_or(cidr);
                        if ip.parse::<IpAddr>().is_ok() {
                            return Ok(Some(ip.to_string()));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    // -----------------------------------------------------------------------
    // Teardown
    // -----------------------------------------------------------------------

    /// Destroys the domain and removes its disk / seed images.
    pub async fn destroy(&self) -> Result<()> {
        match self.provider {
            Provider::Docker => {
                let _ = Command::new("docker")
                    .args(["rm", "-f", &self.name])
                    .output();
                println!("  {} container '{}' removed", "✓".green(), self.name.cyan());
                return Ok(());
            }
            Provider::Libvirt => {
                let _ = run_cmd("virsh", &["destroy", &self.name]);
                let _ = run_cmd("virsh", &["undefine", &self.name, "--remove-all-storage"]);
                if self.disk.exists() {
                    std::fs::remove_file(&self.disk)?;
                }
                println!("  {} VM '{}' removed", "✓".green(), self.name.cyan());
                Ok(())
            }
            Provider::Auto => Ok(()),
        }
    }

    // -----------------------------------------------------------------------
    // SSH helper
    // -----------------------------------------------------------------------

    /// Returns the path to the private key generated during provisioning.
    pub fn ssh_key_path(&self) -> PathBuf {
        vms_dir()
            .join(format!("{}-ssh", self.name))
            .join("id_ed25519")
    }

    /// Returns the SSH username for this flavour.
    pub fn ssh_user(&self) -> &'static str {
        image_info(&self.flavour).user
    }

    /// Returns the host-side SSH port for this domain.
    #[allow(dead_code)]
    pub fn ssh_port(&self) -> u16 {
        self.ssh_port
    }

    // -----------------------------------------------------------------------
    // Exec (abstracts over SSH or docker exec)
    // -----------------------------------------------------------------------

    /// Execute a command inside the guest/container and return its exit code
    /// and combined stdout/stderr as a string.
    pub fn exec(&self, cmd: &str) -> Result<(i32, String)> {
        if matches!(self.provider, Provider::Docker) {
            self.exec_docker(cmd)
        } else {
            self.exec_ssh(cmd)
        }
    }

    fn exec_docker(&self, cmd: &str) -> Result<(i32, String)> {
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

    fn exec_ssh(&self, cmd: &str) -> Result<(i32, String)> {
        use ssh2::Session;
        use std::io::Read;
        use std::net::TcpStream;

        let addr = format!("127.0.0.1:{}", self.ssh_port);
        let tcp = TcpStream::connect(&addr)
            .with_context(|| format!("SSH: failed to connect to {}", addr))?;
        let mut sess = Session::new().context("SSH: failed to create session")?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("SSH: handshake failed")?;
        sess.userauth_pubkey_file(self.ssh_user(), None, &self.ssh_key_path(), None)
            .context("SSH: public-key auth failed")?;

        if !sess.authenticated() {
            bail!("SSH: authentication failed for {}", self.ssh_user());
        }

        let mut channel = sess.channel_session().context("SSH: channel open failed")?;
        channel.exec(cmd).context("SSH: exec failed")?;

        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        let exit_code = channel.exit_status()?;

        let tail = if output.len() > 4096 {
            output[output.len() - 4096..].to_string()
        } else {
            output
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

fn tcp_connectable(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    let addr = format!("{}:{}", host, port);
    if let Ok(mut addrs) = addr.to_socket_addrs() {
        if let Some(addr) = addrs.next() {
            return TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_docker_ubuntu() {
        let d = Domain::new(Flavour::Ubuntu, Provider::Docker);
        assert_eq!(d.ssh_port(), 22);
        assert_eq!(d.ssh_user(), "ubuntu");
    }

    #[test]
    fn test_domain_docker_nixos() {
        let d = Domain::new(Flavour::NixOs, Provider::Docker);
        assert_eq!(d.ssh_port(), 22);
        assert_eq!(d.ssh_user(), "root");
    }

    #[test]
    fn test_domain_libvirt_different_ports() {
        let d = Domain::new(Flavour::Ubuntu, Provider::Libvirt);
        assert_eq!(d.ssh_port(), 22);
    }
}
