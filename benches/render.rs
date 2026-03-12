use lazytail::config::types::StyleValue;
use lazytail::index::flags::{FLAG_FORMAT_JSON, FLAG_FORMAT_LOGFMT};
use lazytail::renderer::preset::{
    compile, RawDetect, RawLayoutEntry, RawPreset, RawStyleCondition,
};
use lazytail::renderer::segment::to_ratatui_style;
use lazytail::renderer::PresetRegistry;
use lazytail::text_wrap::wrap_spans;
use ratatui::text::Span;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const DEFAULT_FILE_SIZE_MB: usize = 100;
const DEFAULT_TRIALS: usize = 10;
const WARMUP_TRIALS: usize = 2;

// ---------------------------------------------------------------------------
// Stats
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

fn lines_per_sec(lines: usize, d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs == 0.0 {
        return "N/A".to_string();
    }
    let rate = lines as f64 / secs;
    if rate >= 1_000_000.0 {
        format!("{:.2}M lines/s", rate / 1_000_000.0)
    } else if rate >= 1_000.0 {
        format!("{:.1}K lines/s", rate / 1_000.0)
    } else {
        format!("{:.0} lines/s", rate)
    }
}

// ---------------------------------------------------------------------------
// Test file generation
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

        // Alternate between JSON and logfmt to test both parsers
        let line = if line_count % 3 == 0 {
            format!(
                r#"{{"level":"{}","message":"{}","service":"{}","timestamp":"{}","request_id":"req-{:06}","duration_ms":{},"extra_field_1":"value_{}","extra_field_2":"another_value_{}"}}
"#,
                lvl,
                msg,
                svc,
                ts,
                line_count,
                line_count % 5000,
                line_count,
                line_count,
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
// Preset builders
// ---------------------------------------------------------------------------

fn build_complex_preset() -> RawPreset {
    let mut style_map = HashMap::new();
    style_map.insert("error".to_string(), "red".to_string());
    style_map.insert("warn".to_string(), "yellow".to_string());
    style_map.insert("info".to_string(), "green".to_string());
    style_map.insert("debug".to_string(), "cyan".to_string());

    RawPreset {
        name: "complex".to_string(),
        parser: Some("json".to_string()),
        detect: Some(RawDetect {
            parser: Some("json".to_string()),
            filename: None,
        }),
        regex: None,
        layout: vec![
            RawLayoutEntry {
                field: Some("timestamp".to_string()),
                literal: None,
                style: Some(StyleValue::Single("dim".to_string())),
                width: Some(20),
                format: Some("datetime:absolute".to_string()),
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: None,
                width: Some(5),
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" | ".to_string()),
                style: Some(StyleValue::Single("dim".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("service".to_string()),
                literal: None,
                style: Some(StyleValue::Single("cyan".to_string())),
                width: Some(15),
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: Some(80),
                style_when: Some(vec![
                    RawStyleCondition {
                        field: Some("level".to_string()),
                        op: "eq".to_string(),
                        value: "error".to_string(),
                        style: StyleValue::Single("red".to_string()),
                    },
                    RawStyleCondition {
                        field: Some("level".to_string()),
                        op: "eq".to_string(),
                        value: "warn".to_string(),
                        style: StyleValue::Single("yellow".to_string()),
                    },
                ]),
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("duration_ms".to_string()),
                literal: None,
                style: Some(StyleValue::Single("dim".to_string())),
                width: None,
                format: Some("duration:ms".to_string()),
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("_rest".to_string()),
                literal: None,
                style: Some(StyleValue::Single("dim".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
        ],
    }
}

fn build_regex_preset() -> RawPreset {
    RawPreset {
        name: "regex-custom".to_string(),
        parser: Some("regex".to_string()),
        detect: None,
        regex: Some(
            r"^(?P<timestamp>\S+)\s+(?P<level>\w+)\s+\[(?P<service>[^\]]+)\]\s+(?P<message>.+)$"
                .to_string(),
        ),
        layout: vec![
            RawLayoutEntry {
                field: Some("timestamp".to_string()),
                literal: None,
                style: Some(StyleValue::Single("dim".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: Some(StyleValue::Single("severity".to_string())),
                width: Some(5),
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("service".to_string()),
                literal: None,
                style: Some(StyleValue::Single("cyan".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: None,
                literal: Some(" ".to_string()),
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
            RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

struct BenchCase {
    name: &'static str,
    registry: PresetRegistry,
    renderer_names: Vec<String>,
    wrap_width: Option<usize>, // None = no wrap, Some(w) = wrap to w columns
}

fn run_bench(case: &BenchCase, lines: &[String], trials: usize) -> Vec<Duration> {
    let total = WARMUP_TRIALS + trials;
    let mut durations = Vec::with_capacity(trials);

    for i in 0..total {
        let start = Instant::now();

        for line in lines {
            if let Some(segments) = case.registry.render_line(line, &case.renderer_names, None) {
                if let Some(width) = case.wrap_width {
                    let spans: Vec<Span<'static>> = segments
                        .into_iter()
                        .map(|seg| Span::styled(seg.text, to_ratatui_style(&seg.style, None)))
                        .collect();
                    std::hint::black_box(wrap_spans(spans, width));
                } else {
                    std::hint::black_box(segments);
                }
            }
        }

        let elapsed = start.elapsed();
        if i >= WARMUP_TRIALS {
            durations.push(elapsed);
        }
    }

    durations
}

fn run_bench_auto(registry: &PresetRegistry, lines: &[String], trials: usize) -> Vec<Duration> {
    let total = WARMUP_TRIALS + trials;
    let mut durations = Vec::with_capacity(trials);

    // Pre-compute flags matching the generation pattern (line_idx % 3):
    //   0 → JSON, 1 → logfmt, 2 → plain text (no format flag)
    let flags_cycle = [Some(FLAG_FORMAT_JSON), Some(FLAG_FORMAT_LOGFMT), Some(0u32)];

    for i in 0..total {
        let start = Instant::now();

        for (idx, line) in lines.iter().enumerate() {
            let flags = flags_cycle[idx % 3];
            std::hint::black_box(registry.render_line_auto(line, None, flags));
        }

        let elapsed = start.elapsed();
        if i >= WARMUP_TRIALS {
            durations.push(elapsed);
        }
    }

    durations
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
    let test_file = tmp_dir.path().join("bench_render.log");

    eprintln!("Generating {} MB test file...", size_mb);
    let (file_bytes, line_count) = generate_test_file(&test_file, size_mb);
    eprintln!("Generated {} ({} lines)", fmt_size(file_bytes), line_count);

    // Load all lines into memory (we're benchmarking rendering, not I/O)
    eprintln!("Loading lines into memory...");
    let lines: Vec<String> = {
        let f = std::fs::File::open(&test_file).expect("open failed");
        std::io::BufReader::new(f)
            .lines()
            .map(|l| l.expect("read line failed"))
            .collect()
    };
    eprintln!("Loaded {} lines", lines.len());

    // Build registries
    let builtin_registry = PresetRegistry::new(Vec::new());

    let cases: Vec<BenchCase> = vec![
        // Builtin JSON, no wrap
        BenchCase {
            name: "builtin_json/no_wrap",
            registry: PresetRegistry::new(Vec::new()),
            renderer_names: vec!["json".to_string()],
            wrap_width: None,
        },
        // Builtin JSON, wrap at 120
        BenchCase {
            name: "builtin_json/wrap_120",
            registry: PresetRegistry::new(Vec::new()),
            renderer_names: vec!["json".to_string()],
            wrap_width: Some(120),
        },
        // Builtin JSON, wrap at 40 (narrow)
        BenchCase {
            name: "builtin_json/wrap_40",
            registry: PresetRegistry::new(Vec::new()),
            renderer_names: vec!["json".to_string()],
            wrap_width: Some(40),
        },
        // Builtin logfmt, no wrap
        BenchCase {
            name: "builtin_logfmt/no_wrap",
            registry: PresetRegistry::new(Vec::new()),
            renderer_names: vec!["logfmt".to_string()],
            wrap_width: None,
        },
        // Builtin logfmt, wrap at 120
        BenchCase {
            name: "builtin_logfmt/wrap_120",
            registry: PresetRegistry::new(Vec::new()),
            renderer_names: vec!["logfmt".to_string()],
            wrap_width: Some(120),
        },
        // Complex preset, no wrap
        BenchCase {
            name: "complex/no_wrap",
            registry: {
                let p = compile(build_complex_preset()).unwrap();
                PresetRegistry::new(vec![p])
            },
            renderer_names: vec!["complex".to_string()],
            wrap_width: None,
        },
        // Complex preset, wrap at 120
        BenchCase {
            name: "complex/wrap_120",
            registry: {
                let p = compile(build_complex_preset()).unwrap();
                PresetRegistry::new(vec![p])
            },
            renderer_names: vec!["complex".to_string()],
            wrap_width: Some(120),
        },
        // Complex preset, wrap at 40
        BenchCase {
            name: "complex/wrap_40",
            registry: {
                let p = compile(build_complex_preset()).unwrap();
                PresetRegistry::new(vec![p])
            },
            renderer_names: vec!["complex".to_string()],
            wrap_width: Some(40),
        },
        // Regex preset, no wrap
        BenchCase {
            name: "regex/no_wrap",
            registry: {
                let p = compile(build_regex_preset()).unwrap();
                PresetRegistry::new(vec![p])
            },
            renderer_names: vec!["regex-custom".to_string()],
            wrap_width: None,
        },
        // Regex preset, wrap at 120
        BenchCase {
            name: "regex/wrap_120",
            registry: {
                let p = compile(build_regex_preset()).unwrap();
                PresetRegistry::new(vec![p])
            },
            renderer_names: vec!["regex-custom".to_string()],
            wrap_width: Some(120),
        },
    ];

    if !json_output {
        println!();
        println!("Render Benchmark");
        println!("================");
        println!("File size:   {}", fmt_size(file_bytes));
        println!("Lines:       {}", lines.len());
        println!(
            "Trials:      {} ({} warmup + {} measured)",
            WARMUP_TRIALS + trials,
            WARMUP_TRIALS,
            trials
        );
        println!();
    }

    let mut json_results = Vec::new();

    for case in &cases {
        eprintln!("  Running {}...", case.name);
        let durations = run_bench(case, &lines, trials);
        let stats = compute_stats(&durations);

        if json_output {
            json_results.push(serde_json::json!({
                "name": case.name,
                "lines": lines.len(),
                "file_bytes": file_bytes,
                "trials": trials,
                "min_ms": stats.min.as_secs_f64() * 1000.0,
                "max_ms": stats.max.as_secs_f64() * 1000.0,
                "mean_ms": stats.mean.as_secs_f64() * 1000.0,
                "stddev_ms": stats.stddev.as_secs_f64() * 1000.0,
                "p50_ms": stats.p50.as_secs_f64() * 1000.0,
                "p95_ms": stats.p95.as_secs_f64() * 1000.0,
                "throughput_mb_per_sec": (file_bytes as f64 / (1024.0 * 1024.0)) / stats.mean.as_secs_f64(),
                "lines_per_sec": lines.len() as f64 / stats.mean.as_secs_f64(),
            }));
        } else {
            println!(
                "  {:<30}  mean: {:>10}  p50: {:>10}  p95: {:>10}  stddev: {:>10}  {}  {}",
                case.name,
                fmt_dur(stats.mean),
                fmt_dur(stats.p50),
                fmt_dur(stats.p95),
                fmt_dur(stats.stddev),
                throughput(file_bytes, stats.mean),
                lines_per_sec(lines.len(), stats.mean),
            );
        }
    }

    // Auto-detect benchmark
    {
        eprintln!("  Running auto_detect...");
        let durations = run_bench_auto(&builtin_registry, &lines, trials);
        let stats = compute_stats(&durations);

        if json_output {
            json_results.push(serde_json::json!({
                "name": "auto_detect",
                "lines": lines.len(),
                "file_bytes": file_bytes,
                "trials": trials,
                "min_ms": stats.min.as_secs_f64() * 1000.0,
                "max_ms": stats.max.as_secs_f64() * 1000.0,
                "mean_ms": stats.mean.as_secs_f64() * 1000.0,
                "stddev_ms": stats.stddev.as_secs_f64() * 1000.0,
                "p50_ms": stats.p50.as_secs_f64() * 1000.0,
                "p95_ms": stats.p95.as_secs_f64() * 1000.0,
                "throughput_mb_per_sec": (file_bytes as f64 / (1024.0 * 1024.0)) / stats.mean.as_secs_f64(),
                "lines_per_sec": lines.len() as f64 / stats.mean.as_secs_f64(),
            }));
        } else {
            println!(
                "  {:<30}  mean: {:>10}  p50: {:>10}  p95: {:>10}  stddev: {:>10}  {}  {}",
                "auto_detect",
                fmt_dur(stats.mean),
                fmt_dur(stats.p50),
                fmt_dur(stats.p95),
                fmt_dur(stats.stddev),
                throughput(file_bytes, stats.mean),
                lines_per_sec(lines.len(), stats.mean),
            );
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json_results).unwrap());
    } else {
        println!();
    }

    // Cleanup happens automatically when tmp_dir drops
}
