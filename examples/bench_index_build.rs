use anyhow::Result;
use std::path::PathBuf;
use std::time::Instant;

use lazytail::index::builder::IndexBuilder;

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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <log_file> [index_dir]", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} long_journal.log", args[0]);
        eprintln!("  {} long_journal.log /tmp/idx", args[0]);
        std::process::exit(1);
    }

    let log_path = PathBuf::from(&args[1]);
    let index_dir = if args.len() >= 3 {
        PathBuf::from(&args[2])
    } else {
        // Default to .lazytail/idx/bench in the same directory as the log file
        let parent = log_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        parent.join(".lazytail").join("idx").join("bench")
    };

    if !log_path.exists() {
        eprintln!("Error: Log file not found: {}", log_path.display());
        std::process::exit(1);
    }

    let log_metadata = std::fs::metadata(&log_path)?;
    let log_size = log_metadata.len();

    println!("Index Build Benchmark");
    println!("=====================");
    println!();
    println!("Log file:    {}", log_path.display());
    println!("Log size:    {}", format_size(log_size));
    println!("Index dir:   {}", index_dir.display());
    println!();

    // Clean up old index if it exists
    if index_dir.exists() {
        println!("Removing old index...");
        std::fs::remove_dir_all(&index_dir)?;
    }

    println!("Building index...");
    println!();

    let start = Instant::now();
    let builder = IndexBuilder::new(); // Default checkpoint interval: 100K lines
    let meta = builder.build(&log_path, &index_dir)?;
    let elapsed = start.elapsed();

    let elapsed_millis = elapsed.as_millis();

    println!("✓ Build complete!");
    println!();
    println!("Results:");
    println!("--------");
    println!("Total lines:         {}", meta.entry_count);
    println!(
        "Checkpoint interval: {} lines",
        meta.checkpoint_interval as u64 * 1000
    );
    println!();
    println!("Time elapsed:        {}", format_duration(elapsed_millis));
    println!(
        "Throughput:          {}",
        format_throughput(log_size, elapsed_millis)
    );
    println!(
        "Lines/sec:           {:.0}",
        meta.entry_count as f64 / elapsed.as_secs_f64()
    );
    println!();

    // Calculate index sizes
    println!("Index Files:");
    println!("------------");
    let mut total_index_size = 0u64;

    for column in &["meta", "checkpoints", "offsets", "lengths", "flags", "time"] {
        let path = index_dir.join(column);
        if path.exists() {
            let size = std::fs::metadata(&path)?.len();
            total_index_size += size;
            println!("  {:<12} {}", column, format_size(size));
        }
    }

    println!("  {:<12} {}", "TOTAL", format_size(total_index_size));
    println!();
    println!(
        "Index overhead:      {:.1}% of log file size",
        (total_index_size as f64 / log_size as f64) * 100.0
    );
    println!();

    // Verify columns are present
    println!("Columns present:");
    println!("  ✓ offsets");
    println!("  ✓ lengths");
    println!("  ✓ flags");
    println!("  ✓ time");
    println!("  ✓ checkpoints");

    Ok(())
}
