//! The store batch: one `store_books` call for a run's whole copy list, and the one
//! measurement pass over the copies that landed.

use std::collections::HashMap;

use library_core::book::Fingerprint;
use library_core::scan::FoundFile;
use library_core::wire::{StoreRequest, StoreResult};

use crate::services::library::{file_name, toast};
use crate::services::library as wire;
use crate::state::AppState;

/// One spelling for both batches the library copies — a folder walk's and a loose file
/// drop's — because a per-file failure is the same news either way. `noun` is what the
/// sentence counts.
fn partition_store_results(
    state: AppState,
    results: Vec<StoreResult>,
    noun: &str,
) -> HashMap<String, String> {
    let mut landed = HashMap::new();
    let mut failures = Vec::new();
    for result in results {
        if result.is_ok() {
            landed.insert(result.id, result.store);
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
) -> Result<HashMap<String, String>, String> {
    let requests: Vec<StoreRequest> = pending
        .iter()
        .map(|(book_id, file)| StoreRequest {
            path: file.path.clone(),
            id: book_id.clone(),
        })
        .collect();
    let results = wire::store_books(task, &requests).await?;
    Ok(partition_store_results(state, results, "files"))
}

/// A copy that cannot be measured is absent, and its row keeps the pending flag the startup sweep finishes.
pub(super) async fn measure_stores(stores: Vec<String>) -> HashMap<String, Fingerprint> {
    if stores.is_empty() {
        return HashMap::new();
    }
    wire::verify_paths(stores)
        .await
        .ok()
        .map(|checks| {
            checks
                .into_iter()
                .filter_map(|check| Some((check.path.clone(), check.fingerprint()?)))
                .collect()
        })
        .unwrap_or_default()
}
