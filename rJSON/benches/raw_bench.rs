use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rjson::{Arena, cjson_parse};
use std::fs;
use std::path::Path;

fn get_payload(name: &str) -> Vec<u8> {
    // Look in /build/bench/inputs, ../bench/inputs/ or bench/inputs/ depending on execution CWD
    let path = if Path::new("/build/bench/inputs").join(name).exists() {
        Path::new("/build/bench/inputs").join(name)
    } else if Path::new("../bench/inputs").join(name).exists() {
        Path::new("../bench/inputs").join(name)
    } else {
        Path::new("bench/inputs").join(name)
    };
    fs::read(path).unwrap_or_else(|_| panic!("Failed to read benchmark input: {}", name))
}

fn raw_parse_benchmarks(c: &mut Criterion) {
    let small = get_payload("small.json");
    let medium = get_payload("medium.json");
    let large = get_payload("large.json");

    c.bench_function("raw_rust/small", |b| {
        b.iter(|| {
            let mut arena = Arena::new();
            let _ = cjson_parse(black_box(&mut arena), black_box(&small)).unwrap();
        })
    });

    c.bench_function("raw_rust/medium", |b| {
        b.iter(|| {
            let mut arena = Arena::new();
            let _ = cjson_parse(black_box(&mut arena), black_box(&medium)).unwrap();
        })
    });

    c.bench_function("raw_rust/large", |b| {
        b.iter(|| {
            let mut arena = Arena::new();
            let _ = cjson_parse(black_box(&mut arena), black_box(&large)).unwrap();
        })
    });
}

criterion_group!(benches, raw_parse_benchmarks);
criterion_main!(benches);
