//! `ConversionSummaryPrinter`: the report printed at the end of every conversion.

use std::path::Path;
use std::time::Duration;

use crate::console::{n0, reset_color, set_color, Color};
use crate::outln;

#[allow(clippy::too_many_arguments)]
pub fn print_summary(
    process_title: &str,
    total_files: usize,
    converted_files: usize,
    skipped_files: usize,
    failed_files: usize,
    total_original_bytes: u64,
    total_output_bytes: u64,
    elapsed: Duration,
    output_directory: &Path,
    format_name: Option<&str>,
) {
    let format_name = format_name.unwrap_or("WebP Lossless");
    let mb = |bytes: f64| bytes / (1024.0 * 1024.0);
    let original = total_original_bytes as i64;
    let output = total_output_bytes as i64;
    let saved = original - output;
    let saving_percent = if original > 0 { saved as f64 / original as f64 * 100.0 } else { 0.0 };
    let seconds = elapsed.as_secs_f64();
    let throughput = if seconds > 0.0 {
        (converted_files + skipped_files + failed_files) as f64 / seconds.max(0.001)
    } else {
        0.0
    };
    let rule = "================================================================================";

    outln!();
    set_color(Color::Cyan);
    outln!("{rule}");
    outln!("                    PROCESS SUMMARY: {}", process_title.to_uppercase());
    outln!("{rule}");
    reset_color();

    outln!("  Format:              {format_name}");
    outln!("  Time Elapsed:        {:.1} min ({seconds:.1} sec)", seconds / 60.0);
    outln!("  Throughput:          {throughput:.1} files/sec");
    outln!("  Files Converted:     {}", n0(converted_files as i64));
    if skipped_files > 0 {
        outln!("  Files Skipped:       {} (already compiled/present)", n0(skipped_files as i64));
    }
    if failed_files > 0 {
        set_color(Color::Yellow);
        outln!("  Files Failed:        {}", n0(failed_files as i64));
        reset_color();
    }
    outln!("  Total Files Scanned: {}", n0(total_files as i64));
    outln!("--------------------------------------------------------------------------------");

    if original > 0 && output > 0 {
        outln!("  Original Size:       {:.2} MB ({} bytes)", mb(original as f64), n0(original));
        outln!("  Output Size:         {:.2} MB ({} bytes)", mb(output as f64), n0(output));
        if saved > 0 {
            set_color(Color::Green);
            outln!("  Disk Space Saved:    {:.2} MB ({saving_percent:.1}% overall reduction)", mb(saved as f64));
            reset_color();
        } else if saved < 0 {
            outln!("  Size Difference:     +{:.2} MB (+{:.1}%)", mb(saved.unsigned_abs() as f64), saving_percent.abs());
        } else {
            outln!("  Disk Space Saved:    0 MB (identical size)");
        }
    } else if output > 0 {
        outln!("  Output Size:         {:.2} MB ({} bytes)", mb(output as f64), n0(output));
    }

    let full = std::path::absolute(output_directory).unwrap_or_else(|_| output_directory.to_path_buf());
    outln!("  Output Directory:    {}", full.display());
    set_color(Color::Cyan);
    outln!("{rule}");
    reset_color();
    outln!();
}
