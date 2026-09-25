//! The store batch: one `store_books` call for a run's whole copy list. The measurement
//! rides home with each copy ([`library_core::wire::StoreResult::measured`] — the shell
//! stamps a copy and reads its head in the same pass), so there is no second verify trip
//! over the copies that landed.

use std::collections::HashMap;

use library_core::book::Fingerprint;
use library_core::scan::FoundFile;
use library_core::wire::{BookFileRequest, StoreResult};

use crate::services::library as ipc;
use crate::services::library::{file_name, toast};
use crate::state::AppState;

/// What a landed copy comes home as: where it stands, and its own measurement (the row
/// adopts it, so the source file's fingerprint stays free for the folder that reads it).
pub(super) type Landed = (String, Option<Fingerprint>);

/// One spelling for both batches the library copies — a folder walk's and a loose file
/// drop's — because a per-file failure is the same news either way. `noun` is what the
/// sentence counts.
pub(crate) fn partition_store_results(
    state: AppState,
    results: Vec<StoreResult>,
    noun: &str,
) -> HashMap<String, Landed> {
    let mut landed = HashMap::new();
    let mut failures = Vec::new();
    for result in results {
        if result.is_ok() {
            landed.insert(result.id, (result.store, result.measured));
        } else {
            failures.push(file_name(&result.src));
        }
    }
    if !failures.is_empty() {
        let message = match failures.len() {
            1 => format!("Could not copy {}", failures[0]),
            n => format!("Could not copy {n} {noun}, starting with {}", failures[0]),
        };
        toast(state, message);
    }
    landed
}

pub(super) async fn copy_batch(
    state: AppState,
    task: &str,
    pending: &[(String, &FoundFile)],
) -> Result<HashMap<String, Landed>, String> {
    let requests: Vec<BookFileRequest> = pending
        .iter()
        .map(|(book_id, file)| BookFileRequest {
            from: file.path.clone(),
            id: book_id.clone(),
        })
        .collect();
    let results = ipc::store_books(task, &requests).await?;
    Ok(partition_store_results(state, results, "files"))
}
