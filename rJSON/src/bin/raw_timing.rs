use rjson::{cjson_parse, Arena};
use std::env;
use std::fs;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <file_path> <iterations> [label]", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];
    let iterations: usize = args[2].parse().expect("Invalid iterations count");
    let label = if args.len() > 3 { &args[3] } else { "raw_rust" };

    let payload = fs::read(path).unwrap_or_else(|_| panic!("Failed to open file: {}", path));

    // Warmup
    for _ in 0..100 {
        let mut arena = Arena::new();
        let _ = cjson_parse(&mut arena, &payload);
    }

    let mut times_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let start = Instant::now();
        let mut arena = Arena::new();
        let res = cjson_parse(black_box(&mut arena), black_box(&payload));
        if res.is_err() {
            eprintln!("Parse failed!");
            std::process::exit(1);
        }
        drop(arena);
        let elapsed = start.elapsed().as_secs_f64() * 1_000_000.0;
        times_us.push(elapsed);
    }

    times_us.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let sum: f64 = times_us.iter().sum();
    let mean = sum / iterations as f64;
    let min = times_us[0];
    let max = times_us[iterations - 1];
    let median = if iterations.is_multiple_of(2) {
        (times_us[iterations / 2 - 1] + times_us[iterations / 2]) / 2.0
    } else {
        times_us[iterations / 2]
    };

    let variance: f64 = times_us.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / iterations as f64;
    let std_dev = variance.sqrt();

    println!(
        "{:<15} | File: {:<15} | Mean: {:8.2} us | Median: {:8.2} us | Min: {:8.2} us | Max: {:8.2} us | StdDev: {:6.2} us | Iters: {}",
        label,
        std::path::Path::new(path).file_name().unwrap().to_str().unwrap(),
        mean, median, min, max, std_dev, iterations
    );

    // Output raw structured JSON line for easy aggregation into bench/results.json
    println!(
        "{{\"api\": \"{}\", \"file\": \"{}\", \"size_bytes\": {}, \"iterations\": {}, \"mean_us\": {:.2}, \"median_us\": {:.2}, \"min_us\": {:.2}, \"max_us\": {:.2}, \"std_dev_us\": {:.2}}}",
        label,
        std::path::Path::new(path).file_name().unwrap().to_str().unwrap(),
        payload.len(),
        iterations,
        mean, median, min, max, std_dev
    );
}
