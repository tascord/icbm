//! Application entry-point that exercises all workspace crates.

use {
    anyhow::Result,
    async_worker::{pipeline, Handler, Ticker},
    compute::{histogram, parallel_sort, pearson, sieve, Matrix},
    models::{Event, Metric, Status, Task, Tree},
    rand::Rng,
    std::{sync::Arc, time::Duration},
    tracing::{info, warn},
    utils::{mean, std_dev, to_camel_case, to_snake_case, Registry},
};

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("app=info".parse()?))
        .init();

    info!("Starting workspace benchmark app");

    // ------------------------------------------------------------------
    // 1. Compute: parallel sort, sieve, matrix multiply
    // ------------------------------------------------------------------
    info!("== Compute phase ==");
    let mut rng = rand::rng();
    let data: Vec<i64> = (0..10_000).map(|_| rng.random_range(-100_000..100_000)).collect();
    let sorted = parallel_sort(&data);
    assert!(sorted.windows(2).all(|w| w[0] <= w[1]));
    info!("Parallel sort OK ({} elements)", sorted.len());

    let primes = sieve(1_000_000);
    info!("Primes up to 1 000 000: {}", primes.len());

    let a = Matrix::from_fn(64, 64, |r, c| (r + c) as f64);
    let b = a.transpose();
    let c = a.mul(&b);
    info!("Matrix 64×64 mul – Frobenius norm: {:.2}", c.frobenius_norm());

    // ------------------------------------------------------------------
    // 2. Statistics
    // ------------------------------------------------------------------
    let xs: Vec<f64> = (0..1000).map(|i| i as f64).collect();
    let ys: Vec<f64> = xs.iter().map(|x| x * 2.0 + 1.0).collect();
    let r = pearson(&xs, &ys);
    assert!((r - 1.0).abs() < 1e-9, "Expected perfect correlation");

    let floats: Vec<f64> = (0..1000).map(|i| i as f64).collect();
    let (_, counts) = histogram(&floats, 10);
    let total: usize = counts.iter().sum();
    assert_eq!(total, 1000);
    info!("Stats OK");

    // ------------------------------------------------------------------
    // 3. Models: tasks, trees, events
    // ------------------------------------------------------------------
    info!("== Models phase ==");
    let mut reg: Registry<Task<u32>> = Registry::new();
    for i in 0..50u32 {
        let name = to_camel_case(&format!("task_{}", i));
        let t = Task::new(name.clone(), i).with_tag("bench").with_meta("index", i.to_string());
        reg.insert(to_snake_case(&name), t);
    }
    info!("Registry has {} tasks", reg.len());

    let tree: Tree<u32> = Tree::leaf(0)
        .with_child(Tree::leaf(1).with_child(Tree::leaf(3)).with_child(Tree::leaf(4)))
        .with_child(Tree::leaf(2).with_child(Tree::leaf(5)));
    info!("Tree depth={} size={}", tree.depth(), tree.size());

    let event: Event<Vec<Metric>> =
        Event::new("bench.complete", vec![Metric::new("cpu_pct", 78.5, "%"), Metric::new("mem_mib", 1024.0, "MiB")]);
    info!("Event id={} kind={}", event.id, event.kind);

    // ------------------------------------------------------------------
    // 4. Async pipeline
    // ------------------------------------------------------------------
    info!("== Async pipeline phase ==");
    let compute_fn: Handler<u32, u64> = Arc::new(|x: u32| {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_micros(10)).await;
            Ok(sieve(x as usize * 10).len() as u64)
        })
    });
    let format_fn: Handler<u64, String> = Arc::new(|n: u64| Box::pin(async move { Ok(format!("{} primes", n)) }));

    let results = pipeline(8, 0u32..100, compute_fn, format_fn).await;
    let ok = results.iter().filter(|r| r.is_ok()).count();
    info!("Pipeline: {}/{} OK", ok, results.len());

    // ------------------------------------------------------------------
    // 5. Ticker
    // ------------------------------------------------------------------
    use futures::StreamExt;
    let ticks: Vec<usize> = Ticker::new(Duration::from_millis(1), 5).stream().collect().await;
    info!("Ticker ticks: {:?}", ticks);

    // ------------------------------------------------------------------
    // 6. Aggregate stats over multiple runs
    // ------------------------------------------------------------------
    let run_times: Vec<f64> = (0..20).map(|i| 1.5 + i as f64 * 0.3).collect();
    info!("Run stats: mean={:.2}s  std_dev={:.2}s", mean(&run_times).unwrap_or(0.0), std_dev(&run_times).unwrap_or(0.0),);

    // ------------------------------------------------------------------
    // 7. Status machine demo
    // ------------------------------------------------------------------
    let mut task: Task<String> = Task::new("final-task", "hello".to_string());
    task.transition(Status::Running).unwrap();
    task.transition(Status::Completed).unwrap();
    if let Err(e) = task.transition(Status::Running) {
        warn!("Expected error: {}", e);
    }

    info!("All phases complete. This machine handled the load.");
    Ok(())
}
