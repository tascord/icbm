//! Async task pipeline built on Tokio.
//!
//! Exercises:
//! * `async fn`, `tokio::spawn`, channels
//! * `futures::stream` combinators
//! * Generic bounds across async boundaries (`Send + Sync + 'static`)
//! * Error propagation with `anyhow`

use {
    anyhow::Result,
    futures::StreamExt,
    models::{Status, Task},
    std::{future::Future, pin::Pin, sync::Arc, time::Duration},
    tokio::{sync::mpsc, time::timeout},
    tracing::{debug, error},
    utils::Id,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A boxed async handler function.
pub type Handler<I, O> = Arc<dyn Fn(I) -> Pin<Box<dyn Future<Output = Result<O>> + Send>> + Send + Sync>;

/// A simple in-process job queue backed by a Tokio MPSC channel.
pub struct JobQueue<I: Send + 'static, O: Send + 'static> {
    tx: mpsc::Sender<Job<I>>,
    results_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<JobResult<O>>>>,
}

struct Job<I> {
    id: Id,
    payload: I,
}

pub struct JobResult<O> {
    pub id: Id,
    pub outcome: Result<O>,
}

impl<I: Send + 'static + Clone, O: Send + 'static> JobQueue<I, O> {
    /// Creates a new queue, spawning `workers` Tokio tasks that consume jobs.
    pub fn new(workers: usize, handler: Handler<I, O>) -> Self {
        let (tx, rx) = mpsc::channel::<Job<I>>(256);
        let (result_tx, result_rx) = mpsc::channel::<JobResult<O>>(256);

        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        for w in 0..workers {
            let rx = Arc::clone(&rx);
            let handler = Arc::clone(&handler);
            let result_tx = result_tx.clone();

            tokio::spawn(async move {
                debug!("Worker {} started", w);
                loop {
                    let job = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    match job {
                        None => {
                            debug!("Worker {} shutting down (channel closed)", w);
                            break;
                        }
                        Some(job) => {
                            let outcome = handler(job.payload).await;
                            if result_tx.send(JobResult { id: job.id, outcome }).await.is_err() {
                                error!("Worker {}: result channel closed", w);
                                break;
                            }
                        }
                    }
                }
            });
        }

        JobQueue { tx, results_rx: Arc::new(tokio::sync::Mutex::new(result_rx)) }
    }

    /// Enqueues a job and returns its ID.
    pub async fn enqueue(&self, payload: I) -> Result<Id> {
        let id = Id::new();
        self.tx.send(Job { id, payload }).await.map_err(|e| anyhow::anyhow!("Failed to enqueue job: {}", e))?;
        Ok(id)
    }

    /// Collects up to `n` results (with a timeout per result).
    pub async fn collect(&self, n: usize, per_result_timeout: Duration) -> Vec<JobResult<O>> {
        let mut results = Vec::with_capacity(n);
        let mut guard = self.results_rx.lock().await;

        for _ in 0..n {
            match timeout(per_result_timeout, guard.recv()).await {
                Ok(Some(r)) => results.push(r),
                _ => break,
            }
        }
        results
    }
}

// ---------------------------------------------------------------------------
// Pipeline – chains two async stages
// ---------------------------------------------------------------------------

/// Connects a producer (`gen`) to a consumer (`consume`) through a bounded
/// channel, yielding all consumer outputs.
///
/// ```ignore
/// let outputs = pipeline(
///     4,
///     (0..100u32),
///     Arc::new(|x| Box::pin(async move { Ok(x * 2) })),
///     Arc::new(|x| Box::pin(async move { Ok(x.to_string()) })),
/// ).await;
/// ```
pub async fn pipeline<I, M, O>(
    concurrency: usize,
    inputs: impl IntoIterator<Item = I> + Send,
    stage1: Handler<I, M>,
    stage2: Handler<M, O>,
) -> Vec<Result<O>>
where
    I: Send + 'static + Clone,
    M: Send + 'static + Clone,
    O: Send + 'static,
{
    let items: Vec<I> = inputs.into_iter().collect();
    let stream = futures::stream::iter(items);

    stream
        .map(|item| {
            let s1 = Arc::clone(&stage1);
            let s2 = Arc::clone(&stage2);
            async move {
                let mid = s1(item).await?;
                s2(mid).await
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await
}

// ---------------------------------------------------------------------------
// Ticker – emits a value every `interval`
// ---------------------------------------------------------------------------

pub struct Ticker {
    interval: Duration,
    limit: usize,
}

impl Ticker {
    pub fn new(interval: Duration, limit: usize) -> Self { Ticker { interval, limit } }

    /// Returns a stream of tick counts.
    pub fn stream(self) -> impl futures::Stream<Item = usize> {
        futures::stream::unfold((0usize, self), |(count, ticker)| async move {
            if count >= ticker.limit {
                return None;
            }
            tokio::time::sleep(ticker.interval).await;
            Some((count, (count + 1, ticker)))
        })
    }
}

// ---------------------------------------------------------------------------
// Task runner – tracks Task<P> status via async channel notifications
// ---------------------------------------------------------------------------

pub struct TaskRunner<P: Clone + serde::Serialize + std::fmt::Debug + Send + 'static> {
    notify_tx: mpsc::Sender<(Id, Status)>,
    pub tasks: Vec<Task<P>>,
}

impl<P: Clone + serde::Serialize + std::fmt::Debug + Send + 'static> TaskRunner<P> {
    pub fn new() -> (Self, mpsc::Receiver<(Id, Status)>) {
        let (tx, rx) = mpsc::channel(64);
        (TaskRunner { notify_tx: tx, tasks: vec![] }, rx)
    }

    pub fn add(&mut self, task: Task<P>) { self.tasks.push(task); }

    pub async fn run_all<F, Fut>(&mut self, f: F)
    where
        F: Fn(P) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        for task in &mut self.tasks {
            let _ = task.transition(Status::Running);
            let _ = self.notify_tx.send((task.id, Status::Running)).await;

            let payload = task.payload.clone();
            let f = f.clone();
            let tx = self.notify_tx.clone();
            let id = task.id;

            tokio::spawn(async move {
                let result = f(payload).await;
                let next = if result.is_ok() { Status::Completed } else { Status::Failed };
                let _ = tx.send((id, next)).await;
            });
        }
    }
}

impl<P: Clone + serde::Serialize + std::fmt::Debug + Send + 'static> Default for TaskRunner<P> {
    fn default() -> Self { Self::new().0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Arc};

    #[tokio::test]
    async fn test_pipeline() {
        let double: Handler<u32, u32> = Arc::new(|x| Box::pin(async move { Ok(x * 2) }));
        let stringify: Handler<u32, String> = Arc::new(|x| Box::pin(async move { Ok(x.to_string()) }));

        let results = pipeline(4, 0u32..5, double, stringify).await;
        let values: Vec<String> = results.into_iter().map(|r| r.unwrap()).collect();
        let mut parsed: Vec<u32> = values.iter().map(|s| s.parse().unwrap()).collect();
        parsed.sort();
        assert_eq!(parsed, vec![0, 2, 4, 6, 8]);
    }

    #[tokio::test]
    async fn test_ticker() {
        use futures::StreamExt;
        let ticks: Vec<usize> = Ticker::new(Duration::from_millis(1), 3).stream().collect().await;
        assert_eq!(ticks, vec![0, 1, 2]);
    }
}
