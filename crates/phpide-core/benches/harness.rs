//! Benchmark sem dependências externas (harness próprio, estável no Rust
//! stable): mediana de `iters` execuções após aquecimento.
//! Resultados registrados em `docs/baseline/T-NFR-006-02-perf-baseline.md`.

use std::time::{Duration, Instant};

pub fn bench<F: FnMut()>(name: &str, iters: usize, warmup: usize, mut f: F) {
    for _ in 0..warmup {
        f();
    }
    let mut samples: Vec<Duration> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        f();
        samples.push(t0.elapsed());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    let total: Duration = samples.iter().sum();
    let ops = if total.as_nanos() > 0 {
        (iters as f64) / total.as_secs_f64()
    } else {
        0.0
    };
    println!("{name:<38} mediana={:>8.1?}  ~{ops:>10.0} ops/s", median);
}
