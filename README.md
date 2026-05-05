# ICBM – Integrated Container BenchMark

**icbm** provisions Ubuntu and NixOS virtual machines via libvirt/virsh,
installs Rust inside each VM, builds a complex multi-crate workspace, and
records CPU usage, memory usage and wall-clock time for every step.  The
results are combined into a single score (0–1000) that indicates how suitable
the host machine is for serious Rust development.

---

## Quick start (one-liner)

```sh
curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
```

The script will:
1. Install Rust via `rustup` if not already present.
2. Check that `virsh`, `virt-install` and `qemu-img` are available (and print
   installation hints if they are not).
3. Sparse-clone only the tool source from this repository.
4. Build `icbm` in release mode.
5. Run the full benchmark suite.

---

## Manual build

```sh
# Prerequisites: Rust stable, libvirt / qemu-kvm, virt-install, qemu-img
git clone --filter=blob:none --sparse https://github.com/tascord/icbm
cd icbm
git sparse-checkout set src Cargo.toml Cargo.lock
cargo build --release
./target/release/icbm --help
```

---

## Usage

```
icbm [OPTIONS] [COMMAND]

Commands:
  run        Run the full benchmark suite (default)
  host-info  Print host machine information (no VMs)

Options (run):
  --flavours <LIST>   Comma-separated VM flavours to benchmark [default: ubuntu,nixos]
  --keep-vms          Keep VMs alive after the benchmark
  --json-out <PATH>   Write JSON report to file
  --skip-provision    Skip VM creation (reuse existing domains)
  -h, --help          Print help
  -V, --version       Print version
```

### Examples

```sh
# Benchmark Ubuntu only, save JSON report
icbm run --flavours ubuntu --json-out report.json

# Show host info without running VMs
icbm host-info

# Reuse already-provisioned VMs
icbm run --skip-provision --keep-vms
```

---

## Benchmark steps

| # | Step | What it measures |
|---|------|-----------------|
| 1 | **Install Rust** | rustup download + toolchain install speed |
| 2 | **Clone workspace** | git sparse-checkout speed |
| 3 | **`cargo clippy --fix`** | incremental analysis speed |
| 4 | **`cargo build` (dev)** | debug build throughput |
| 5 | **`cargo build --release`** | optimised build throughput |

Each step is wrapped in a sampler that records:
* Wall-clock elapsed time
* Average & peak system CPU utilisation (%)
* Peak system memory usage (MiB)

---

## Scoring

| Dimension | Weight |
|-----------|--------|
| Wall-clock time (lower = better) | 40 % |
| CPU utilisation (higher sustained = better) | 30 % |
| Peak memory usage (lower = better) | 30 % |

Scores are normalised against generous reference ceilings (600 s / step,
8 GiB RAM) so that typical machines land in the 400–800 range.

| Score range | Rating |
|-------------|--------|
| 900–1000 | ⚡ Exceptional |
| 750–899  | 🚀 Excellent   |
| 550–749  | ✅ Good        |
| 350–549  | ⚠️  Adequate   |
| 0–349    | ❌ Poor        |

---

## Repository layout

```
icbm/
├── src/               # Benchmarking tool (Rust)
│   ├── main.rs        # CLI entry-point
│   ├── vm.rs          # virsh VM provisioning
│   ├── bench.rs       # SSH benchmark runner
│   ├── metrics.rs     # CPU / memory / time sampler
│   ├── score.rs       # Scoring algorithm
│   └── report.rs      # Terminal + JSON reporting
├── workspace/         # Complex multi-crate Rust workspace (benchmark target)
│   ├── Cargo.toml     # Workspace root
│   └── crates/
│       ├── utils/     # String, numeric, generic utilities
│       ├── models/    # Generic domain models (Task, Tree, Event)
│       ├── compute/   # CPU-intensive algorithms (sort, matrix, primes)
│       ├── async-worker/ # Tokio pipeline and job queue
│       └── app/       # Binary tying all crates together
├── install.sh         # One-line installer
└── Cargo.toml         # Tool manifest
```

---

## Prerequisites

| Tool | Purpose |
|------|---------|
| `virsh` / `libvirt` | VM lifecycle management |
| `virt-install` | VM creation |
| `qemu-img` | Overlay disk creation |
| `cloud-localds` | cloud-init seed ISO |
| `ssh-keygen` | VM SSH key generation |
| `curl` / `git` | Image download + repo clone |

**Debian/Ubuntu:** `sudo apt install qemu-kvm libvirt-daemon-system virtinst cloud-image-utils`  
**Fedora/RHEL:** `sudo dnf install qemu-kvm libvirt virt-install cloud-utils`  
**NixOS:** `nix-env -iA nixpkgs.libvirt nixpkgs.virt-manager nixpkgs.cloud-utils`
