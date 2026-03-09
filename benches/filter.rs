use lazytail::filter::cancel::CancelToken;
use lazytail::filter::engine::FilterProgress;
use lazytail::filter::query::{self, QueryFilter};
use lazytail::filter::regex_filter::RegexFilter;
use lazytail::filter::search_engine::SearchEngine;
use lazytail::filter::streaming_filter;
use lazytail::filter::string_filter::StringFilter;
use lazytail::filter::Filter;
use lazytail::index::builder::IndexBuilder;
use lazytail::index::reader::IndexReader;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const DEFAULT_FILE_SIZE_MB: usize = 100;
const DEFAULT_TRIALS: usize = 10;
const WARMUP_TRIALS: usize = 2;

// ---------------------------------------------------------------------------
// Stats (shared with render bench)
// ---------------------------------------------------------------------------

struct Stats {
    min: Duration,
    max: Duration,
    mean: Duration,
    stddev: Duration,
    p50: Duration,
    p95: Duration,
}

fn compute_stats(durations: &[Duration]) -> Stats {
    let mut sorted: Vec<Duration> = durations.to_vec();
    sorted.sort();

    let total_nanos: u128 = sorted.iter().map(|d| d.as_nanos()).sum();
    let mean_nanos = total_nanos / sorted.len() as u128;
    let mean = Duration::from_nanos(mean_nanos as u64);

    let variance: f64 = sorted
        .iter()
        .map(|d| {
            let diff = d.as_nanos() as f64 - mean_nanos as f64;
            diff * diff
        })
        .sum::<f64>()
        / sorted.len() as f64;
    let stddev = Duration::from_nanos(variance.sqrt() as u64);

    let p50_idx = ((sorted.len() - 1) as f64 * 0.50).round() as usize;
    let p95_idx = ((sorted.len() - 1) as f64 * 0.95).round() as usize;

    Stats {
        min: sorted[0],
        max: *sorted.last().unwrap(),
        mean,
        stddev,
        p50: sorted[p50_idx],
        p95: sorted[p95_idx],
    }
}

fn fmt_dur(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{:.1} us", ms * 1000.0)
    } else if ms < 1000.0 {
        format!("{:.2} ms", ms)
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

fn fmt_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn throughput(bytes: u64, d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs == 0.0 {
        return "N/A".to_string();
    }
    let mb_per_sec = (bytes as f64 / (1024.0 * 1024.0)) / secs;
    format!("{:.1} MB/s", mb_per_sec)
}

// ---------------------------------------------------------------------------
// Test file generation (same mixed format as render bench)
// ---------------------------------------------------------------------------

fn generate_test_file(path: &Path, target_size_mb: usize) -> (u64, usize) {
    let target_bytes = target_size_mb * 1024 * 1024;
    let mut f = std::fs::File::create(path).expect("failed to create test file");
    let mut written = 0u64;
    let mut line_count = 0usize;

    let services = ["api", "web", "worker", "db", "auth", "gateway", "cache"];
    let levels = ["error", "warn", "info", "debug", "info", "info", "info"];
    let messages = [
        "connection refused to upstream service at 10.0.0.5:8080",
        "slow query detected, duration exceeded threshold",
        "request completed successfully",
        "loading configuration from environment",
        "user authentication token validated",
        "cache miss for key user:session:abc-123-def-456",
        "health check passed, all dependencies available",
        "rate limit approaching threshold for client",
        "database connection pool exhausted, waiting for available connection",
        "TLS handshake completed with certificate verification",
    ];

    while (written as usize) < target_bytes {
        let svc = services[line_count % services.len()];
        let lvl = levels[line_count % levels.len()];
        let msg = messages[line_count % messages.len()];
        let ts = format!(
            "2024-01-15T10:{:02}:{:02}.{:03}Z",
            (line_count / 3600) % 60,
            (line_count / 60) % 60,
            line_count % 1000,
        );

        let line = if line_count % 3 == 0 {
            format!(
                r#"{{"level":"{}","message":"{}","service":"{}","timestamp":"{}","request_id":"req-{:06}","duration_ms":{},"extra_field_1":"value_{}","extra_field_2":"another_value_{}"}}
"#,
                lvl, msg, svc, ts, line_count, line_count % 5000, line_count, line_count,
            )
        } else if line_count % 3 == 1 {
            format!(
                "level={} msg=\"{}\" service={} ts={} request_id=req-{:06} duration_ms={} extra_1=val_{}\n",
                lvl, msg, svc, ts, line_count, line_count % 5000, line_count,
            )
        } else {
            format!(
                "{} {} [{}] {} request_id=req-{:06} duration={}ms\n",
                ts,
                lvl.to_uppercase(),
                svc,
                msg,
                line_count,
                line_count % 5000,
            )
        };

        let bytes = line.as_bytes();
        f.write_all(bytes).expect("write failed");
        written += bytes.len() as u64;
        line_count += 1;
    }
    f.flush().expect("flush failed");

    (written, line_count)
}

// ---------------------------------------------------------------------------
// Helper: drain receiver and count matches
// ---------------------------------------------------------------------------

fn collect_matches(rx: std::sync::mpsc::Receiver<FilterProgress>) -> (usize, usize) {
    let mut match_count = 0;
    let mut lines_processed = 0;
    for progress in rx {
        match progress {
            FilterProgress::PartialResults { matches, .. } => {
                match_count += matches.len();
            }
            FilterProgress::Complete {
                matches,
                lines_processed: lp,
            } => {
                match_count += matches.len();
                lines_processed = lp;
                break;
            }
            FilterProgress::Error(e) => panic!("Filter error: {}", e),
            FilterProgress::Processing(_) => {}
        }
    }
    (match_count, lines_processed)
}

// ---------------------------------------------------------------------------
// Benchmark cases
// ---------------------------------------------------------------------------

struct FilterBenchCase {
    name: &'static str,
    run: Box<dyn Fn(&Path) -> (usize, usize)>,
}

fn make_cases(test_file: &Path) -> Vec<FilterBenchCase> {
    let path = test_file.to_path_buf();

    let p = path.clone();
    let string_ci = FilterBenchCase {
        name: "string/case_insensitive",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> =
                Arc::new(StringFilter::new("connection refused", false));
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let string_cs = FilterBenchCase {
        name: "string/case_sensitive",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> =
                Arc::new(StringFilter::new("connection refused", true));
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let string_fast = FilterBenchCase {
        name: "string/simd_fast_path",
        run: Box::new(move |_| {
            let rx = SearchEngine::search_file_fast(
                &p,
                b"connection refused",
                false,
                CancelToken::new(),
            )
            .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let string_rare = FilterBenchCase {
        name: "string/rare_pattern",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> =
                Arc::new(StringFilter::new("TLS handshake", false));
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let regex_simple = FilterBenchCase {
        name: "regex/simple",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> =
                Arc::new(RegexFilter::new(r"error|warn", false).unwrap());
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let regex_complex = FilterBenchCase {
        name: "regex/complex",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> = Arc::new(
                RegexFilter::new(
                    r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z.*(error|warn)",
                    false,
                )
                .unwrap(),
            );
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let query_json = FilterBenchCase {
        name: "query/json_level_eq",
        run: Box::new(move |_| {
            let fq = query::parse_query(r#"json | level == "error""#).unwrap();
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let query_logfmt = FilterBenchCase {
        name: "query/logfmt_level_eq",
        run: Box::new(move |_| {
            let fq = query::parse_query("logfmt | level == error").unwrap();
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let query_multi = FilterBenchCase {
        name: "query/json_multi_filter",
        run: Box::new(move |_| {
            let fq = query::parse_query(r#"json | level == "error" | service == "api""#).unwrap();
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter(p.clone(), filter, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    // Index-accelerated cases
    let p = path.clone();
    let indexed_json = FilterBenchCase {
        name: "indexed/json_level_eq",
        run: Box::new(move |_| {
            let fq = query::parse_query(r#"json | level == "error""#).unwrap();
            let (mask, want) = fq.index_mask().unwrap();
            let reader = IndexReader::open(&p).unwrap();
            let bitmap = reader.candidate_bitmap(mask, want, reader.len());
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter_indexed(
                p.clone(),
                filter,
                bitmap,
                CancelToken::new(),
            )
            .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let indexed_multi = FilterBenchCase {
        name: "indexed/json_multi_filter",
        run: Box::new(move |_| {
            let fq = query::parse_query(r#"json | level == "error" | service == "api""#).unwrap();
            let (mask, want) = fq.index_mask().unwrap();
            let reader = IndexReader::open(&p).unwrap();
            let bitmap = reader.candidate_bitmap(mask, want, reader.len());
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter_indexed(
                p.clone(),
                filter,
                bitmap,
                CancelToken::new(),
            )
            .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let indexed_logfmt = FilterBenchCase {
        name: "indexed/logfmt_level_eq",
        run: Box::new(move |_| {
            let fq = query::parse_query("logfmt | level == error").unwrap();
            let (mask, want) = fq.index_mask().unwrap();
            let reader = IndexReader::open(&p).unwrap();
            let bitmap = reader.candidate_bitmap(mask, want, reader.len());
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq).unwrap());
            let rx = streaming_filter::run_streaming_filter_indexed(
                p.clone(),
                filter,
                bitmap,
                CancelToken::new(),
            )
            .unwrap();
            collect_matches(rx)
        }),
    };

    // SearchEngine dispatch (the actual path used by the app)
    let p = path.clone();
    let engine_string = FilterBenchCase {
        name: "engine/string_dispatch",
        run: Box::new(move |_| {
            let filter: Arc<dyn Filter> =
                Arc::new(StringFilter::new("connection refused", false));
            let rx = SearchEngine::search_file(&p, filter, None, None, None, CancelToken::new())
                .unwrap();
            collect_matches(rx)
        }),
    };

    let p = path.clone();
    let engine_query_indexed = FilterBenchCase {
        name: "engine/query_indexed",
        run: Box::new(move |_| {
            let fq = query::parse_query(r#"json | level == "error""#).unwrap();
            let reader = IndexReader::open(&p);
            let filter: Arc<dyn Filter> = Arc::new(QueryFilter::new(fq.clone()).unwrap());
            let rx = SearchEngine::search_file(
                &p,
                filter,
                Some(&fq),
                reader.as_ref(),
                None,
                CancelToken::new(),
            )
            .unwrap();
            collect_matches(rx)
        }),
    };

    vec![
        string_ci,
        string_cs,
        string_fast,
        string_rare,
        regex_simple,
        regex_complex,
        query_json,
        query_logfmt,
        query_multi,
        indexed_json,
        indexed_multi,
        indexed_logfmt,
        engine_string,
        engine_query_indexed,
    ]
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let size_mb = args
        .iter()
        .find(|a| a.starts_with("--size="))
        .and_then(|a| a.strip_prefix("--size=")?.parse().ok())
        .unwrap_or(DEFAULT_FILE_SIZE_MB);

    let trials = args
        .iter()
        .find(|a| a.starts_with("--trials="))
        .and_then(|a| a.strip_prefix("--trials=")?.parse().ok())
        .unwrap_or(DEFAULT_TRIALS);

    let json_output = args.iter().any(|a| a == "--json");

    // Generate test file
    let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let test_file = tmp_dir.path().join("bench_filter.log");

    eprintln!("Generating {} MB test file...", size_mb);
    let (file_bytes, line_count) = generate_test_file(&test_file, size_mb);
    eprintln!(
        "Generated {} ({} lines)",
        fmt_size(file_bytes),
        line_count
    );

    // Build index for indexed benchmarks
    eprintln!("Building index...");
    let idx_dir = test_file.with_extension("idx");
    let idx_start = Instant::now();
    IndexBuilder::new()
        .with_checkpoint_interval(100)
        .build(&test_file, &idx_dir)
        .expect("failed to build index");
    let idx_elapsed = idx_start.elapsed();
    eprintln!(
        "Index built in {} ({})",
        fmt_dur(idx_elapsed),
        throughput(file_bytes, idx_elapsed)
    );

    // Build bench cases
    let cases = make_cases(&test_file);

    if !json_output {
        println!();
        println!("Filter Benchmark");
        println!("================");
        println!("File size:   {}", fmt_size(file_bytes));
        println!("Lines:       {}", line_count);
        println!(
            "Trials:      {} ({} warmup + {} measured)",
            WARMUP_TRIALS + trials,
            WARMUP_TRIALS,
            trials
        );
        println!("Index build: {}", fmt_dur(idx_elapsed));
        println!();
    }

    let mut json_results = Vec::new();

    for case in &cases {
        eprintln!("  Running {}...", case.name);

        let total = WARMUP_TRIALS + trials;
        let mut durations = Vec::with_capacity(trials);
        let mut last_match_count = 0;
        let mut last_lines_processed = 0;

        for i in 0..total {
            let start = Instant::now();
            let (match_count, lines_processed) = (case.run)(&test_file);
            let elapsed = start.elapsed();

            if i >= WARMUP_TRIALS {
                durations.push(elapsed);
                last_match_count = match_count;
                last_lines_processed = lines_processed;
            }
        }

        let stats = compute_stats(&durations);

        if json_output {
            json_results.push(serde_json::json!({
                "name": case.name,
                "lines": line_count,
                "file_bytes": file_bytes,
                "trials": trials,
                "matches": last_match_count,
                "lines_processed": last_lines_processed,
                "min_ms": stats.min.as_secs_f64() * 1000.0,
                "max_ms": stats.max.as_secs_f64() * 1000.0,
                "mean_ms": stats.mean.as_secs_f64() * 1000.0,
                "stddev_ms": stats.stddev.as_secs_f64() * 1000.0,
                "p50_ms": stats.p50.as_secs_f64() * 1000.0,
                "p95_ms": stats.p95.as_secs_f64() * 1000.0,
                "throughput_mb_per_sec": (file_bytes as f64 / (1024.0 * 1024.0)) / stats.mean.as_secs_f64(),
            }));
        } else {
            println!(
                "  {:<30}  mean: {:>10}  p50: {:>10}  p95: {:>10}  stddev: {:>10}  {}  matches: {}",
                case.name,
                fmt_dur(stats.mean),
                fmt_dur(stats.p50),
                fmt_dur(stats.p95),
                fmt_dur(stats.stddev),
                throughput(file_bytes, stats.mean),
                last_match_count,
            );
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_results).unwrap()
        );
    } else {
        println!();
    }
}
