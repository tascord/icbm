//! Real-time CPU / memory / wall-clock metrics collector.
//!
//! `Sampler` spawns a background Tokio task that polls `sysinfo` every
//! `SAMPLE_INTERVAL_MS` milliseconds and accumulates statistics.  Call
//! `Sampler::start()` before a benchmark step, and `Sampler::finish()` after
//! to obtain a `StepMetrics` snapshot.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use sysinfo::System;
use tokio::{task::JoinHandle, time::sleep};

const SAMPLE_INTERVAL_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Public data structures
// ---------------------------------------------------------------------------

/// Metrics captured for one benchmark step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepMetrics {
    /// Wall-clock duration in seconds.
    pub elapsed_secs: f64,
    /// Peak RSS of the whole system in MiB.
    pub peak_memory_mib: u64,
    /// Average system-wide CPU utilisation (0–100 %).
    pub avg_cpu_pct: f64,
    /// Peak system-wide CPU utilisation (0–100 %).
    pub peak_cpu_pct: f64,
    /// Start timestamp (UTC).
    pub started_at: DateTime<Utc>,
    /// End timestamp (UTC).
    pub finished_at: DateTime<Utc>,
}

/// Basic host information captured once at start-up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub cpu_brand: String,
    pub cpu_count: usize,
    pub total_memory_mib: u64,
}

// ---------------------------------------------------------------------------
// host_info
// ---------------------------------------------------------------------------

pub fn host_info() -> HostInfo {
    let mut sys = System::new_all();
    sys.refresh_all();

    HostInfo {
        hostname: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        cpu_brand: sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default(),
        cpu_count: sys.cpus().len(),
        total_memory_mib: sys.total_memory() / 1024 / 1024,
    }
}

// ---------------------------------------------------------------------------
// Sampler
// ---------------------------------------------------------------------------

/// Collects resource metrics while a benchmark step runs.
///
/// Usage:
/// ```ignore
/// let sampler = Sampler::start();
/// // … do work …
/// let metrics = sampler.finish().await?;
/// ```
pub struct Sampler {
    shared: Arc<Mutex<Shared>>,
    handle: JoinHandle<()>,
    wall_start: Instant,
    wall_started_at: DateTime<Utc>,
}

#[derive(Default)]
struct Shared {
    samples: Vec<CpuMemSample>,
    stopped: bool,
}

#[derive(Clone)]
struct CpuMemSample {
    cpu_pct: f64,
    mem_mib: u64,
}

impl Sampler {
    /// Starts background sampling immediately.
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let shared_clone = Arc::clone(&shared);

        let handle = tokio::spawn(async move {
            let mut sys = System::new();
            loop {
                {
                    let guard = shared_clone.lock().unwrap();
                    if guard.stopped {
                        break;
                    }
                }

                sys.refresh_cpu_all();
                sys.refresh_memory();

                let cpu_pct = sys.global_cpu_usage() as f64;
                let mem_mib = sys.used_memory() / 1024 / 1024;

                {
                    let mut guard = shared_clone.lock().unwrap();
                    guard.samples.push(CpuMemSample { cpu_pct, mem_mib });
                }

                sleep(Duration::from_millis(SAMPLE_INTERVAL_MS)).await;
            }
        });

        Sampler {
            shared,
            handle,
            wall_start: Instant::now(),
            wall_started_at: Utc::now(),
        }
    }

    /// Stops background sampling and returns aggregated metrics.
    pub async fn finish(self) -> Result<StepMetrics> {
        let elapsed = self.wall_start.elapsed();
        let finished_at = Utc::now();

        {
            let mut guard = self.shared.lock().unwrap();
            guard.stopped = true;
        }

        // Give the sampling loop one last chance to see `stopped = true`.
        self.handle.await?;

        let guard = self.shared.lock().unwrap();
        let samples = &guard.samples;

        let peak_memory_mib = samples.iter().map(|s| s.mem_mib).max().unwrap_or(0);

        let cpu_sum: f64 = samples.iter().map(|s| s.cpu_pct).sum();
        let count = samples.len().max(1) as f64;
        let avg_cpu_pct = cpu_sum / count;
        let peak_cpu_pct = samples
            .iter()
            .map(|s| s.cpu_pct)
            .fold(f64::NEG_INFINITY, f64::max);
        let peak_cpu_pct = if peak_cpu_pct.is_infinite() {
            0.0
        } else {
            peak_cpu_pct
        };

        Ok(StepMetrics {
            elapsed_secs: elapsed.as_secs_f64(),
            peak_memory_mib,
            avg_cpu_pct,
            peak_cpu_pct,
            started_at: self.wall_started_at,
            finished_at,
        })
    }
}
