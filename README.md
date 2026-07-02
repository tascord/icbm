# ICBM – Integrated Container BenchMark

this is a hyper specific tool for benchmarking 86x rust performance inside vms on different machines. its probably not super useful if you aren't me or someone in my team

#### + [submit](https://github.com/tascord/icbm/issues/new?template=build-submission.md) new scores

### run
```bash
curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
```

The installer checks for Docker (preferred provider) and will attempt to
install it on macOS (via Homebrew) or Linux (via the system package manager).
If Docker is unavailable, `auto` will fall back to libvirt on Linux.

### build
```
# Linux prerequisites: Rust stable, libvirt / qemu-kvm, virt-install, qemu-img
# macOS prerequisites: Rust stable, Docker Desktop
git clone --filter=blob:none --sparse https://github.com/tascord/icbm
cd icbm
git sparse-checkout set src Cargo.toml Cargo.lock
cargo build --release
./target/release/icbm --help
```

### usage
```
icbm [OPTIONS] [COMMAND]

Commands:
  run        Run the full benchmark suite (default)
  host-info  Print host machine information (no VMs)

Options (run):
  --flavours <LIST>   Comma-separated VM flavours to benchmark [default: ubuntu,nixos]
  --provider <NAME>   VM provider: auto, libvirt, docker [default: auto]
  --keep-vms          Keep VMs alive after the benchmark
  --json-out <PATH>   Write JSON report to file
  --skip-provision    Skip VM creation (reuse existing domains)
  -h, --help          Print help
  -V, --version       Print version
```

### benchmark
| #   | Step                              | What it measures                          |
| --- | --------------------------------- | ----------------------------------------- |
| 1   | Install Rust                      | rustup download + toolchain install speed |
| 2   | Clone workspace                   | git sparse-checkout speed                 |
| 3   | `cargo clippy --fix`              | incremental analysis speed                |
| 4   | `cargo build` (dev)               | debug build throughput                    |
| 5   | `cargo build --release`           | optimised build throughput                |
| 6   | Link (dev)                        | incremental linking speed after edit      |
| 7   | Link (release)                    | incremental linking speed after edit      |

Each step is wrapped in a sampler that records:
  - Wall-clock elapsed time
  - Average & peak system CPU utilisation (%)
  - Peak system memory usage (MiB)

Scores are normalised against generous reference ceilings (600 s / step, 8 GiB RAM) so that typical machines land in the 400–800 range.
