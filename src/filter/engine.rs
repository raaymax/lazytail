use super::Filter;
use crate::reader::LogReader;
use anyhow::Result;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Filter progress update
#[derive(Debug, Clone)]
pub enum FilterProgress {
    /// Currently processing (lines processed so far)
    Processing(usize),
    /// Filtering complete (matching line indices)
    Complete(Vec<usize>),
    /// Error occurred
    Error(String),
}

/// Filter engine that processes filters in the background
pub struct FilterEngine;

impl FilterEngine {
    /// Run a filter on a log reader in a background thread
    /// Returns a receiver for progress updates
    pub fn run_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        progress_interval: usize,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            if let Err(e) = Self::process_filter(reader, filter, tx.clone(), progress_interval) {
                let _ = tx.send(FilterProgress::Error(e.to_string()));
            }
        });

        rx
    }

    /// Internal filter processing
    fn process_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        progress_interval: usize,
    ) -> Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let total_lines = {
            let reader = reader.lock().unwrap();
            reader.total_lines()
        };

        let mut matching_indices = Vec::new();

        for line_idx in 0..total_lines {
            // Get the line
            let line = {
                let mut reader = reader.lock().unwrap();
                match reader.get_line(line_idx)? {
                    Some(line) => line,
                    None => continue,
                }
            };

            // Check if it matches
            if filter.matches(&line) {
                matching_indices.push(line_idx);
            }

            // Send progress update periodically
            if line_idx % progress_interval == 0 {
                tx.send(FilterProgress::Processing(line_idx))?;
            }
        }

        // Send final results
        tx.send(FilterProgress::Complete(matching_indices))?;

        Ok(())
    }
}
