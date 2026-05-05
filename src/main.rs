use {
    anyhow::Result,
    clap::{Parser, Subcommand},
    colored::Colorize,
};

mod bench;
mod metrics;
mod report;
mod score;
mod vm;

/// ICBM – Integrated Container BenchMark
///
/// Provisions Ubuntu and NixOS VMs, installs Rust, runs a complex multi-crate
/// build inside each VM while recording CPU / memory / wall-clock time, then
/// assigns a score that indicates how suitable the host machine is for a
/// prospective Rust developer.
#[derive(Parser, Debug)]
#[command(
    name = "icbm",
    version,
    about = "Integrated Container BenchMark – rate a machine for Rust development",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the full benchmark suite (default when no subcommand is given).
    Run {
        /// VM flavour(s) to benchmark.  Comma-separated list: ubuntu,nixos
        #[arg(long, default_value = "ubuntu,nixos")]
        flavours: String,

        /// Keep the VMs alive after the benchmark finishes.
        #[arg(long, default_value_t = false)]
        keep_vms: bool,

        /// Path at which to write the JSON report.  Defaults to stdout.
        #[arg(long)]
        json_out: Option<std::path::PathBuf>,

        /// Skip VM provisioning; assume virsh domains already exist with the
        /// given names and that SSH is reachable via the addresses printed
        /// during a previous run.
        #[arg(long, default_value_t = false)]
        skip_provision: bool,
    },

    /// Print information about the host machine (no VMs, no benchmark).
    HostInfo,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("\n{}  {}\n", "🚀 ICBM".bold().cyan(), "Integrated Container BenchMark".dimmed());

    match cli.command.unwrap_or(Commands::Run {
        flavours: "ubuntu,nixos".to_string(),
        keep_vms: false,
        json_out: None,
        skip_provision: false,
    }) {
        Commands::Run { flavours, keep_vms, json_out, skip_provision } => {
            let requested: Vec<vm::Flavour> = flavours.split(',').map(|s| s.trim().parse()).collect::<Result<_, _>>()?;

            run_benchmark(requested, keep_vms, json_out, skip_provision).await?;
        }

        Commands::HostInfo => {
            let info = metrics::host_info();
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
    }

    Ok(())
}

async fn run_benchmark(
    flavours: Vec<vm::Flavour>,
    keep_vms: bool,
    json_out: Option<std::path::PathBuf>,
    skip_provision: bool,
) -> Result<()> {
    let mut all_results = vec![];

    for flavour in &flavours {
        println!("\n{} {}", "▶  Benchmarking flavour:".bold(), flavour.to_string().yellow().bold());

        // ------------------------------------------------------------------
        // 1. Provision VM (unless caller asked to skip)
        // ------------------------------------------------------------------
        let domain = vm::Domain::new(flavour.clone());

        if !skip_provision {
            domain.provision().await?;
        }

        let ip = domain.wait_for_ip().await?;
        println!("   VM IP: {}", ip.green());

        // ------------------------------------------------------------------
        // 2. Run benchmark steps inside the VM, collecting metrics
        // ------------------------------------------------------------------
        let result = bench::run(&domain, &ip).await?;

        // ------------------------------------------------------------------
        // 3. Teardown (unless --keep-vms)
        // ------------------------------------------------------------------
        if !keep_vms {
            domain.destroy().await.ok(); // best-effort
        }

        // ------------------------------------------------------------------
        // 4. Score
        // ------------------------------------------------------------------
        let scored = score::evaluate(flavour.clone(), result);
        report::print_summary(&scored);
        all_results.push(scored);
    }

    // Global leaderboard
    report::print_leaderboard(&all_results);

    // Optional JSON output
    if let Some(path) = json_out {
        let json = serde_json::to_string_pretty(&all_results)?;
        std::fs::write(&path, json)?;
        println!("\n{} {}", "📄 JSON report written to".dimmed(), path.display().to_string().underline());
    }

    Ok(())
}
