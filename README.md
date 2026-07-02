# ICBM – Integrated Container BenchMark

this is a hyper specific tool for benchmarking rust build performance inside docker containers on different machines. because containers run the host architecture, arm machines benchmark arm rust and x86_64 machines benchmark x86_64 rust. its probably not super useful if you aren't me or someone in my team

#### + [submit](https://github.com/tascord/icbm/issues/new?template=build-submission.md) new scores

### run
```bash
curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
```

The installer downloads the `icbm` binary and, on Linux, will also download the
Docker CLI static binary if `docker` is not already in your `PATH`. On macOS you
need Docker Desktop, Colima, or Podman Machine running before running the
installer — it will not install Docker for you.

### build
```
# Prerequisites: Rust stable, Docker (or Colima / Podman on macOS)
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
  host-info  Print host machine information (no containers)

Options (run):
  --flavours <LIST>   Comma-separated flavours to benchmark [default: ubuntu,nixos]
  --keep-vms          Keep containers alive after the benchmark
  --json-out <PATH>   Write JSON report to file
  --skip-provision    Skip container creation (reuse existing containers)
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
