use super::BenchArgs;
use crate::filter::cancel::CancelToken;
use crate::filter::engine::FilterProgress;
use crate::filter::query::{self, FilterQuery, QueryFilter};
use crate::filter::regex_filter::RegexFilter;
use crate::filter::search_engine::SearchEngine;
use crate::filter::string_filter::StringFilter;
use crate::filter::Filter;
use crate::reader::file_reader::FileReader;
use crate::reader::LogReader;
use lazytail::index::reader::IndexReader;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct BenchResult {
    durations: Vec<Duration>,
    matches: usize,
    lines_searched: usize,
}

struct TrialStats {
    min: Duration,
    max: Duration,
    mean: Duration,
    stddev: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
}

pub fn run(args: BenchArgs) -> Result<(), i32> {
    // Validate files exist
    for file in &args.files {
        if !file.exists() {
            eprintln!("Error: File not found: {}", file.display());
            return Err(1);
        }
    }

    // Clamp trials to minimum 2 (1 warmup + 1 measured)
    let trials = args.trials.max(2);

    // Determine filter mode label
    let mode_label = if args.query {
        "query"
    } else if args.regex {
        "regex"
    } else {
        "plain"
    };

    // Build filter (and optionally parse query)
    let (filter, filter_query) = match build_filter(&args.pattern, &args) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Err(1);
        }
    };

    let mut json_results = if args.json { Some(Vec::new()) } else { None };

    for file in &args.files {
        let file_size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let total_lines = match FileReader::new(file) {
            Ok(r) => r.total_lines(),
            Err(e) => {
                eprintln!("Error: Failed to open {}: {}", file.display(), e);
                return Err(1);
            }
        };

        if args.compare {
            // Run non-indexed path
            let non_indexed =
                match run_trials(file, filter.clone(), filter_query.as_ref(), None, trials) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error benchmarking {}: {}", file.display(), e);
                        return Err(1);
                    }
                };
            let non_indexed_stats = compute_stats(&non_indexed.durations);

            // Try indexed path
            let index = IndexReader::open(file);
            if let Some(ref idx) = index {
                let indexed = match run_trials(
                    file,
                    filter.clone(),
                    filter_query.as_ref(),
                    Some(idx),
                    trials,
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error benchmarking {}: {}", file.display(), e);
                        return Err(1);
                    }
                };
                let indexed_stats = compute_stats(&indexed.durations);

                if let Some(ref mut results) = json_results {
                    results.push(build_compare_json(
                        file,
                        file_size,
                        total_lines,
                        &args.pattern,
                        mode_label,
                        trials,
                        &non_indexed,
                        &non_indexed_stats,
                        &indexed,
                        &indexed_stats,
                    ));
                } else {
                    print_compare_results(
                        file,
                        file_size,
                        total_lines,
                        &args.pattern,
                        mode_label,
                        trials,
                        &non_indexed,
                        &non_indexed_stats,
                        &indexed,
                        &indexed_stats,
                    );
                }
            } else {
                eprintln!(
                    "Note: No index found for {}. Running non-indexed path only.",
                    file.display()
                );
                if let Some(ref mut results) = json_results {
                    results.push(build_result_json(
                        file,
                        file_size,
                        total_lines,
                        &args.pattern,
                        mode_label,
                        trials,
                        &non_indexed,
                        &non_indexed_stats,
                    ));
                } else {
                    print_results(
                        file,
                        file_size,
                        total_lines,
                        &args.pattern,
                        mode_label,
                        trials,
                        &non_indexed,
                        &non_indexed_stats,
                    );
                }
            }
        } else {
            let index = IndexReader::open(file);
            let result = match run_trials(
                file,
                filter.clone(),
                filter_query.as_ref(),
                index.as_ref(),
                trials,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error benchmarking {}: {}", file.display(), e);
                    return Err(1);
                }
            };
            let stats = compute_stats(&result.durations);

            if let Some(ref mut results) = json_results {
                results.push(build_result_json(
                    file,
                    file_size,
                    total_lines,
                    &args.pattern,
                    mode_label,
                    trials,
                    &result,
                    &stats,
                ));
            } else {
                print_results(
                    file,
                    file_size,
                    total_lines,
                    &args.pattern,
                    mode_label,
                    trials,
                    &result,
                    &stats,
                );
            }
        }
    }

    if let Some(results) = json_results {
        print_json(&serde_json::Value::Array(results));
    }

    Ok(())
}

fn build_filter(
    pattern: &str,
    args: &BenchArgs,
) -> Result<(Arc<dyn Filter>, Option<FilterQuery>), String> {
    if args.query {
        let filter_query = query::parse_query(pattern).map_err(|e| format!("{}", e))?;
        let query_filter = QueryFilter::new(filter_query.clone())?;
        Ok((Arc::new(query_filter), Some(filter_query)))
    } else if args.regex {
        let filter = RegexFilter::new(pattern, args.case_sensitive)
            .map_err(|e| format!("Invalid regex pattern: {}", e))?;
        Ok((Arc::new(filter), None))
    } else {
        Ok((
            Arc::new(StringFilter::new(pattern, args.case_sensitive)),
            None,
        ))
    }
}

fn run_trials(
    path: &Path,
    filter: Arc<dyn Filter>,
    query: Option<&FilterQuery>,
    index: Option<&IndexReader>,
    trials: usize,
) -> Result<BenchResult, String> {
    let mut durations = Vec::with_capacity(trials - 1);
    let mut last_matches = 0;
    let mut last_lines_searched = 0;

    for i in 0..trials {
        let start = Instant::now();

        let rx =
            SearchEngine::search_file(path, filter.clone(), query, index, None, CancelToken::new())
                .map_err(|e| format!("Search failed: {}", e))?;

        let (matches, lines_searched) = collect_filter_results(rx)?;
        let elapsed = start.elapsed();

        last_matches = matches.len();
        last_lines_searched = lines_searched;

        // Discard warmup trial (first one)
        if i > 0 {
            durations.push(elapsed);
        }
    }

    Ok(BenchResult {
        durations,
        matches: last_matches,
        lines_searched: last_lines_searched,
    })
}

fn collect_filter_results(rx: Receiver<FilterProgress>) -> Result<(Vec<usize>, usize), String> {
    let mut matching_indices = Vec::new();
    let mut lines_searched = 0;

    for progress in rx {
        match progress {
            FilterProgress::PartialResults {
                matches,
                lines_processed,
            } => {
                matching_indices.extend(matches);
                lines_searched = lines_processed;
            }
            FilterProgress::Complete {
                matches,
                lines_processed,
            } => {
                matching_indices.extend(matches);
                lines_searched = lines_processed;
            }
            FilterProgress::Processing(n) => {
                lines_searched = n;
            }
            FilterProgress::Error(e) => return Err(e),
        }
    }

    Ok((matching_indices, lines_searched))
}

fn compute_stats(durations: &[Duration]) -> TrialStats {
    if durations.is_empty() {
        return TrialStats {
            min: Duration::ZERO,
            max: Duration::ZERO,
            mean: Duration::ZERO,
            stddev: Duration::ZERO,
            p50: Duration::ZERO,
            p95: Duration::ZERO,
            p99: Duration::ZERO,
        };
    }

    let mut sorted: Vec<Duration> = durations.to_vec();
    sorted.sort();

    let min = sorted[0];
    let max = sorted[sorted.len() - 1];

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

    let p50 = percentile(&sorted, 50);
    let p95 = percentile(&sorted, 95);
    let p99 = percentile(&sorted, 99);

    TrialStats {
        min,
        max,
        mean,
        stddev,
        p50,
        p95,
        p99,
    }
}

fn percentile(sorted: &[Duration], pct: usize) -> Duration {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = (pct as f64 / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[allow(clippy::too_many_arguments)]
fn print_results(
    path: &Path,
    file_size: u64,
    total_lines: usize,
    pattern: &str,
    mode_label: &str,
    trials: usize,
    result: &BenchResult,
    stats: &TrialStats,
) {
    println!("Filter Benchmark");
    println!("================");
    println!();
    println!("File:        {}", path.display());
    println!("File size:   {}", format_size(file_size));
    println!("Total lines: {}", total_lines);
    println!("Pattern:     {}", pattern);
    println!("Mode:        {}", mode_label);
    println!(
        "Trials:      {} (1 warmup + {} measured)",
        trials,
        trials - 1
    );
    println!();
    println!("Results:");
    println!("--------");
    println!("Matches:     {}", result.matches);
    if result.lines_searched > 0 {
        println!(
            "Match ratio: {:.1}%",
            result.matches as f64 / result.lines_searched as f64 * 100.0
        );
    }
    println!();
    println!("Timing:");
    println!("  min:    {}", format_duration(stats.min.as_millis()));
    println!("  max:    {}", format_duration(stats.max.as_millis()));
    println!("  mean:   {}", format_duration(stats.mean.as_millis()));
    println!("  stddev: {}", format_duration(stats.stddev.as_millis()));
    println!("  p50:    {}", format_duration(stats.p50.as_millis()));
    println!("  p95:    {}", format_duration(stats.p95.as_millis()));
    println!("  p99:    {}", format_duration(stats.p99.as_millis()));
    println!();
    println!("Throughput:");
    println!("  {}", format_throughput(file_size, stats.mean.as_millis()));
    println!("  {}", format_rate(total_lines, stats.mean.as_millis()));
    println!();
}

#[allow(clippy::too_many_arguments)]
fn print_compare_results(
    path: &Path,
    file_size: u64,
    total_lines: usize,
    pattern: &str,
    mode_label: &str,
    trials: usize,
    non_indexed: &BenchResult,
    non_indexed_stats: &TrialStats,
    indexed: &BenchResult,
    indexed_stats: &TrialStats,
) {
    println!("Filter Benchmark (Compare Mode)");
    println!("===============================");
    println!();
    println!("File:        {}", path.display());
    println!("File size:   {}", format_size(file_size));
    println!("Total lines: {}", total_lines);
    println!("Pattern:     {}", pattern);
    println!("Mode:        {}", mode_label);
    println!(
        "Trials:      {} (1 warmup + {} measured)",
        trials,
        trials - 1
    );
    println!();
    println!("Non-indexed:");
    println!("  Matches:  {}", non_indexed.matches);
    println!(
        "  mean:     {}",
        format_duration(non_indexed_stats.mean.as_millis())
    );
    println!(
        "  p50:      {}",
        format_duration(non_indexed_stats.p50.as_millis())
    );
    println!(
        "  p95:      {}",
        format_duration(non_indexed_stats.p95.as_millis())
    );
    println!();
    println!("Indexed:");
    println!("  Matches:  {}", indexed.matches);
    println!(
        "  mean:     {}",
        format_duration(indexed_stats.mean.as_millis())
    );
    println!(
        "  p50:      {}",
        format_duration(indexed_stats.p50.as_millis())
    );
    println!(
        "  p95:      {}",
        format_duration(indexed_stats.p95.as_millis())
    );
    println!();

    let speedup = if indexed_stats.mean.as_nanos() > 0 {
        non_indexed_stats.mean.as_nanos() as f64 / indexed_stats.mean.as_nanos() as f64
    } else {
        0.0
    };
    println!("Speedup:     {:.2}x (mean)", speedup);
    println!();
}

#[allow(clippy::too_many_arguments)]
fn build_result_json(
    path: &Path,
    file_size: u64,
    total_lines: usize,
    pattern: &str,
    mode_label: &str,
    trials: usize,
    result: &BenchResult,
    stats: &TrialStats,
) -> serde_json::Value {
    serde_json::json!({
        "file": path.display().to_string(),
        "file_size": file_size,
        "total_lines": total_lines,
        "pattern": pattern,
        "mode": mode_label,
        "trials": trials - 1,
        "matches": result.matches,
        "lines_searched": result.lines_searched,
        "timing": {
            "min_ms": stats.min.as_millis(),
            "max_ms": stats.max.as_millis(),
            "mean_ms": stats.mean.as_millis(),
            "stddev_ms": stats.stddev.as_millis(),
            "p50_ms": stats.p50.as_millis(),
            "p95_ms": stats.p95.as_millis(),
            "p99_ms": stats.p99.as_millis(),
        },
        "throughput_bytes_per_sec": if stats.mean.as_millis() > 0 {
            (file_size as f64 / stats.mean.as_millis() as f64) * 1000.0
        } else {
            0.0
        },
        "throughput_lines_per_sec": if stats.mean.as_millis() > 0 {
            (total_lines as f64 / stats.mean.as_millis() as f64) * 1000.0
        } else {
            0.0
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn build_compare_json(
    path: &Path,
    file_size: u64,
    total_lines: usize,
    pattern: &str,
    mode_label: &str,
    trials: usize,
    non_indexed: &BenchResult,
    non_indexed_stats: &TrialStats,
    indexed: &BenchResult,
    indexed_stats: &TrialStats,
) -> serde_json::Value {
    let speedup = if indexed_stats.mean.as_nanos() > 0 {
        non_indexed_stats.mean.as_nanos() as f64 / indexed_stats.mean.as_nanos() as f64
    } else {
        0.0
    };

    serde_json::json!({
        "file": path.display().to_string(),
        "file_size": file_size,
        "total_lines": total_lines,
        "pattern": pattern,
        "mode": mode_label,
        "trials": trials - 1,
        "non_indexed": {
            "matches": non_indexed.matches,
            "lines_searched": non_indexed.lines_searched,
            "timing": {
                "min_ms": non_indexed_stats.min.as_millis(),
                "max_ms": non_indexed_stats.max.as_millis(),
                "mean_ms": non_indexed_stats.mean.as_millis(),
                "stddev_ms": non_indexed_stats.stddev.as_millis(),
                "p50_ms": non_indexed_stats.p50.as_millis(),
                "p95_ms": non_indexed_stats.p95.as_millis(),
                "p99_ms": non_indexed_stats.p99.as_millis(),
            },
        },
        "indexed": {
            "matches": indexed.matches,
            "lines_searched": indexed.lines_searched,
            "timing": {
                "min_ms": indexed_stats.min.as_millis(),
                "max_ms": indexed_stats.max.as_millis(),
                "mean_ms": indexed_stats.mean.as_millis(),
                "stddev_ms": indexed_stats.stddev.as_millis(),
                "p50_ms": indexed_stats.p50.as_millis(),
                "p95_ms": indexed_stats.p95.as_millis(),
                "p99_ms": indexed_stats.p99.as_millis(),
            },
        },
        "speedup": speedup,
    })
}

fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "[]".to_string())
    );
}

fn format_duration(millis: u128) -> String {
    if millis < 1000 {
        format!("{} ms", millis)
    } else {
        format!("{:.2} s", millis as f64 / 1000.0)
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

fn format_throughput(bytes: u64, millis: u128) -> String {
    if millis == 0 {
        return "N/A".to_string();
    }
    let bytes_per_sec = (bytes as f64 / millis as f64) * 1000.0;
    format_size(bytes_per_sec as u64) + "/s"
}

fn format_rate(lines: usize, millis: u128) -> String {
    if millis == 0 {
        return "N/A".to_string();
    }
    let lines_per_sec = (lines as f64 / millis as f64) * 1000.0;
    format!("{:.0} lines/s", lines_per_sec)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn default_args() -> BenchArgs {
        BenchArgs {
            pattern: String::new(),
            files: vec![],
            regex: false,
            query: false,
            case_sensitive: false,
            trials: 5,
            json: false,
            compare: false,
        }
    }

    #[test]
    fn test_compute_stats_basic() {
        let durations = vec![
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(30),
            Duration::from_millis(40),
            Duration::from_millis(50),
        ];
        let stats = compute_stats(&durations);
        assert_eq!(stats.min, Duration::from_millis(10));
        assert_eq!(stats.max, Duration::from_millis(50));
        assert_eq!(stats.mean, Duration::from_millis(30));
        assert_eq!(stats.p50, Duration::from_millis(30));
    }

    #[test]
    fn test_compute_stats_single_trial() {
        let durations = vec![Duration::from_millis(42)];
        let stats = compute_stats(&durations);
        assert_eq!(stats.min, Duration::from_millis(42));
        assert_eq!(stats.max, Duration::from_millis(42));
        assert_eq!(stats.mean, Duration::from_millis(42));
        assert_eq!(stats.p50, Duration::from_millis(42));
        assert_eq!(stats.p95, Duration::from_millis(42));
        assert_eq!(stats.p99, Duration::from_millis(42));
    }

    #[test]
    fn test_build_filter_plain() {
        let mut args = default_args();
        args.pattern = "error".to_string();
        let (filter, query) = build_filter("error", &args).unwrap();
        assert!(query.is_none());
        assert!(filter.matches("contains error here"));
        assert!(!filter.matches("no match here"));
    }

    #[test]
    fn test_build_filter_regex() {
        let mut args = default_args();
        args.regex = true;
        let (filter, query) = build_filter("err(or|no)", &args).unwrap();
        assert!(query.is_none());
        assert!(filter.matches("error"));
        assert!(filter.matches("errno"));
    }

    #[test]
    fn test_build_filter_invalid_regex() {
        let mut args = default_args();
        args.regex = true;
        let result = build_filter("[invalid", &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_filter_query() {
        let mut args = default_args();
        args.query = true;
        let (_, query) = build_filter("json | level == \"error\"", &args).unwrap();
        assert!(query.is_some());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(500), "500 ms");
        assert_eq!(format_duration(1500), "1.50 s");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
    }

    #[test]
    fn test_collect_filter_results_complete() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(FilterProgress::Complete {
            matches: vec![0, 5, 10],
            lines_processed: 100,
        })
        .unwrap();
        drop(tx);

        let (matches, lines) = collect_filter_results(rx).unwrap();
        assert_eq!(matches, vec![0, 5, 10]);
        assert_eq!(lines, 100);
    }

    #[test]
    fn test_collect_filter_results_partial_then_complete() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(FilterProgress::PartialResults {
            matches: vec![0, 5],
            lines_processed: 50,
        })
        .unwrap();
        tx.send(FilterProgress::Complete {
            matches: vec![10],
            lines_processed: 100,
        })
        .unwrap();
        drop(tx);

        let (matches, lines) = collect_filter_results(rx).unwrap();
        assert_eq!(matches, vec![0, 5, 10]);
        assert_eq!(lines, 100);
    }

    #[test]
    fn test_collect_filter_results_error() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(FilterProgress::Error("fail".to_string())).unwrap();
        drop(tx);

        let result = collect_filter_results(rx);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "fail");
    }
}
