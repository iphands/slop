# rayon - Data Parallelism Library

Work-stealing thread pool for easy data parallelism in Rust.

## Core Concept
Convert sequential iterators to parallel with `.par_iter()`:
```rust
use rayon::prelude::*;
let sum: i32 = data.par_iter().map(|x| x * 2).sum();
```

## Common Patterns

### Parallel Iteration
```rust
use rayon::prelude::*;

// Immutable iteration
pixels.par_iter().for_each(|pixel| process(pixel));

// Mutable iteration
pixels.par_iter_mut().for_each(|pixel| *pixel = transform(*pixel));

// Collecting results
let results: Vec<_> = data.par_iter().map(|x| expensive(x)).collect();
```

### Parallel Chunks
```rust
// Process in chunks (better cache locality)
pixels.par_chunks_mut(64).for_each(|chunk| {
    for pixel in chunk {
        *pixel = process(*pixel);
    }
});
```

### Parallel Indexing
```rust
(0..height).into_par_iter().for_each(|y| {
    for x in 0..width {
        let idx = y * width + x;
        pixels[idx] = render_pixel(x, y);
    }
});
```

### Ray Tracing Pattern
```rust
use rayon::prelude::*;

fn render_parallel(width: usize, height: usize) -> Vec<Color> {
    let pixels = (0..width * height)
        .into_par_iter()
        .map(|idx| {
            let x = idx % width;
            let y = idx / width;
            cast_ray(x, y)
        })
        .collect();
    pixels
}
```

## Configuration

### Thread Pool
```rust
use rayon::ThreadPoolBuilder;

let pool = ThreadPoolBuilder::new()
    .num_threads(8)
    .build()
    .unwrap();

pool.install(|| {
    // Parallel work happens here
    data.par_iter().for_each(process);
});
```

### Global Pool
```rust
// Set global thread count (call once at startup)
rayon::ThreadPoolBuilder::new()
    .num_threads(num_cpus::get())
    .build_global()
    .unwrap();
```

## Performance Tips
- Overhead for very small datasets - profile before parallelizing
- Use `.par_chunks()` for better cache locality
- Avoid false sharing (separate data by cache line size)
- Parallel overhead ~50-100µs, ensure work > 1ms per item
- Use sequential for <1000 items unless work is expensive

## Common Use Cases
- Image processing (per-pixel operations)
- Ray tracing (per-ray independent)
- Batch transformations
- Monte Carlo simulations
- Data analysis/aggregation

## Avoiding Race Conditions
```rust
use std::sync::Mutex;

let counter = Mutex::new(0);
data.par_iter().for_each(|item| {
    // Do work...
    let mut c = counter.lock().unwrap();
    *c += 1;
});

// Better: use reduce instead
let count = data.par_iter().map(|_| 1).sum::<usize>();
```
