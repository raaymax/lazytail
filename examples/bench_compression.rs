use anyhow::Result;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;

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

fn compress_lz4(data: &[u8]) -> Vec<u8> {
    let max_compressed_size = lz4_flex::block::get_maximum_output_size(data.len());
    let mut compressed = vec![0u8; max_compressed_size];
    let compressed_size = lz4_flex::block::compress_into(data, &mut compressed).unwrap();
    compressed.truncate(compressed_size);
    compressed
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <log_file> [frame_size]", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} long_journal.log 1000", args[0]);
        std::process::exit(1);
    }

    let log_path = PathBuf::from(&args[1]);
    let frame_size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);

    if !log_path.exists() {
        eprintln!("Error: Log file not found: {}", log_path.display());
        std::process::exit(1);
    }

    let log_metadata = std::fs::metadata(&log_path)?;
    let original_size = log_metadata.len();

    println!("Log Compression Benchmark");
    println!("=========================");
    println!();
    println!("File:       {}", log_path.display());
    println!("Size:       {}", format_size(original_size));
    println!("Frame size: {} lines", frame_size);
    println!();
    println!("Analyzing compression ratios...");
    println!();

    let file = File::open(&log_path)?;
    let reader = BufReader::new(file);

    let mut total_original = 0u64;
    let mut total_compressed = 0u64;
    let mut frame_buffer = String::new();
    let mut line_count = 0u64;
    let mut frame_count = 0u64;

    let start = Instant::now();

    for line in reader.lines() {
        let line = line?;
        frame_buffer.push_str(&line);
        frame_buffer.push('\n');
        line_count += 1;

        if line_count % frame_size as u64 == 0 {
            let original = frame_buffer.len() as u64;
            let compressed = compress_lz4(frame_buffer.as_bytes());
            let compressed_len = compressed.len() as u64;

            total_original += original;
            total_compressed += compressed_len;
            frame_count += 1;

            // Progress every 100 frames
            if frame_count % 100 == 0 {
                let ratio = total_original as f64 / total_compressed as f64;
                println!(
                    "  Processed {} frames ({} lines) - Ratio: {:.2}x",
                    frame_count, line_count, ratio
                );
            }

            frame_buffer.clear();
        }

        // Limit to 1M lines for speed
        if line_count >= 1_000_000 {
            println!("  (Stopping at 1M lines for quick benchmark)");
            break;
        }
    }

    // Handle last partial frame
    if !frame_buffer.is_empty() {
        let original = frame_buffer.len() as u64;
        let compressed = compress_lz4(frame_buffer.as_bytes());
        total_original += original;
        total_compressed += compressed.len() as u64;
        frame_count += 1;
    }

    let elapsed = start.elapsed();

    println!();
    println!("Results:");
    println!("--------");
    println!("Lines analyzed:      {}", line_count);
    println!("Frames created:      {}", frame_count);
    println!();
    println!("Original size:       {}", format_size(total_original));
    println!("Compressed size:     {}", format_size(total_compressed));
    println!(
        "Compression ratio:   {:.2}x",
        total_original as f64 / total_compressed as f64
    );
    println!(
        "Space saved:         {} ({:.1}%)",
        format_size(total_original - total_compressed),
        ((total_original - total_compressed) as f64 / total_original as f64) * 100.0
    );
    println!();
    println!("Time elapsed:        {:.2}s", elapsed.as_secs_f64());
    println!(
        "Compression speed:   {}/s",
        format_size((total_original as f64 / elapsed.as_secs_f64()) as u64)
    );
    println!();

    // Extrapolate to full file
    if line_count < original_size / 100 {
        let full_file_lines = (original_size as f64 / total_original as f64) * line_count as f64;
        let estimated_compressed = (total_compressed as f64 / line_count as f64) * full_file_lines;
        println!("Estimated for full file:");
        println!(
            "  Compressed size:   {}",
            format_size(estimated_compressed as u64)
        );
        println!(
            "  Space saved:       {}",
            format_size(original_size - estimated_compressed as u64)
        );
    }

    Ok(())
}
