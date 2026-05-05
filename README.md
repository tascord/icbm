# ICBM – Integrated Container BenchMark

this is like a hyper specific tool for benchmarking 86x rust performance inside vms on different machines.

### run
```bash
curl -sSf https://raw.githubusercontent.com/tascord/icbm/main/install.sh | sh
```

### build
```
# Prerequisites: Rust stable, libvirt / qemu-kvm, virt-install, qemu-img
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
  --keep-vms          Keep VMs alive after the benchmark
  --json-out <PATH>   Write JSON report to file
  --skip-provision    Skip VM creation (reuse existing domains)
  -h, --help          Print help
  -V, --version       Print version
```

### benchmark
Benchmark steps
| #   | Step                  | What it measures                          |
| --- | --------------------- | ----------------------------------------- |
| 1   | Install Rust          | rustup download + toolchain install speed |
| 2   | Clone workspace       | git sparse-checkout speed                 |
| 3   | cargo clippy --fix    | incremental analysis speed                |
| 4   | cargo build (dev)     | debug build throughput                    |
| 5   | cargo build --release | optimised build throughput                |

Each step is wrapped in a sampler that records:
  - Wall-clock elapsed time
  - Average & peak system CPU utilisation (%)
  - Peak system memory usage (MiB)

Scores are normalised against generous reference ceilings (600 s / step, 8 GiB RAM) so that typical machines land in the 400–800 range.
