//! What to fetch, where it lands, and what proves it landed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Progress;
use crate::names::{file_name_from_url, safe_file_name};
use crate::partial::partial_of;

/// The check a completed body must pass before it is adopted.
pub type Verify = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// A per-download progress hook, called outside every lock.
pub type ProgressHook = Arc<dyn Fn(&Progress) + Send + Sync>;

/// One download: where from, where to, what proves it landed.
#[derive(Clone)]
pub struct Job {
    pub(crate) id: String,
    pub(crate) sources: Vec<String>,
    pub(crate) dir: PathBuf,
    pub(crate) file_name: Option<String>,
    pub(crate) force: bool,
    pub(crate) verify: Option<Verify>,
    pub(crate) on_progress: Option<ProgressHook>,
}

impl Job {
    /// A download of `url` into `<dir>/<the url's own file name>`.
    pub fn new(id: impl Into<String>, dir: impl Into<PathBuf>, url: impl AsRef<str>) -> Self {
        Self {
            id: id.into(),
            sources: vec![url.as_ref().to_string()],
            dir: dir.into(),
            file_name: None,
            force: false,
            verify: None,
            on_progress: None,
        }
    }

    /// One more mirror of the same file, tried after the ones already listed.
    pub fn mirror(mut self, url: impl AsRef<str>) -> Self {
        self.sources.push(url.as_ref().to_string());
        self
    }

    /// Every mirror at once, in the order they should be tried.
    pub fn mirrors<I, S>(mut self, urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.sources
            .extend(urls.into_iter().map(|url| url.as_ref().to_string()));
        self
    }

    /// The name the file is adopted as, overriding the URL's own.
    pub fn named(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// Download even when the destination is already complete.
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// The check a complete body must pass; a failure discards it.
    pub fn verified_by<F>(mut self, check: F) -> Self
    where
        F: Fn(&Path) -> Result<(), String> + Send + Sync + 'static,
    {
        self.verify = Some(Arc::new(check));
        self
    }

    /// A second progress sink beside the host's own.
    pub fn on_progress<F>(mut self, hook: F) -> Self
    where
        F: Fn(&Progress) + Send + Sync + 'static,
    {
        self.on_progress = Some(Arc::new(hook));
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The mirrors, in the order they are tried.
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The destination, or `None` when no URL names a file.
    pub fn dest(&self) -> Option<PathBuf> {
        Some(self.dir.join(self.file_name()?))
    }

    /// The partial this job's transfer writes into.
    pub fn partial(&self) -> Option<PathBuf> {
        self.dest().map(|dest| partial_of(&dest))
    }

    /// The name the file lands as: the given one, else the first URL's.
    pub fn file_name(&self) -> Option<String> {
        match &self.file_name {
            Some(name) => safe_file_name(name),
            None => self.sources.first().and_then(|url| file_name_from_url(url)),
        }
    }

    /// Whether every field names something this crate can act on.
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("a download needs an id".into());
        }
        if self.sources.is_empty() {
            return Err(format!("download '{}' has no source", self.id));
        }
        for url in &self.sources {
            let scheme = url.split("://").next().unwrap_or("");
            if !matches!(scheme, "http" | "https") {
                return Err(format!("download '{}' fetches a '{scheme}' url", self.id));
            }
        }
        if self.file_name().is_none() {
            return Err(format!("download '{}' names no file", self.id));
        }
        if self.dir.as_os_str().is_empty() {
            return Err(format!("download '{}' has no directory", self.id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_needs_nothing_but_an_id_a_directory_and_a_url() {
        let job = Job::new("cefr", "/data/cefr", "https://host/a/cefr.parquet");
        assert_eq!(job.id(), "cefr");
        assert_eq!(job.sources().len(), 1);
        assert_eq!(job.dest(), Some(PathBuf::from("/data/cefr/cefr.parquet")));
        assert_eq!(
            job.partial(),
            Some(PathBuf::from("/data/cefr/cefr.parquet.part"))
        );
        assert!(job.check().is_ok());
    }

    #[test]
    fn mirrors_keep_their_order_and_a_name_overrides_the_url() {
        let job = Job::new("m", "/tmp", "https://a/one.bin")
            .mirror("https://b/two.bin")
            .mirrors(["https://c/three.bin"])
            .named("chosen.bin");
        assert_eq!(
            job.sources(),
            &[
                "https://a/one.bin".to_string(),
                "https://b/two.bin".to_string(),
                "https://c/three.bin".to_string()
            ]
        );
        assert_eq!(job.dest(), Some(PathBuf::from("/tmp/chosen.bin")));
    }

    #[test]
    fn a_url_that_names_no_file_is_refused() {
        let job = Job::new("m", "/tmp", "https://host/data/");
        assert!(job.file_name().is_none());
        assert!(job.check().is_err());
        // An explicit name rescues it.
        assert!(job.named("body.bin").check().is_ok());
    }

    #[test]
    fn a_name_that_would_escape_its_directory_is_refused() {
        assert!(
            Job::new("m", "/tmp", "https://h/a")
                .named("../evil")
                .check()
                .is_err()
        );
        assert!(
            Job::new("m", "/tmp", "https://h/a")
                .named("/etc/passwd")
                .check()
                .is_err()
        );
    }

    #[test]
    fn only_http_and_https_are_fetched() {
        assert!(
            Job::new("m", "/tmp/x.bin", "file:///etc/passwd")
                .check()
                .is_err()
        );
        assert!(
            Job::new("m", "/tmp/x.bin", "ftp://host/x.bin")
                .check()
                .is_err()
        );
        assert!(Job::new("m", "/tmp", "http://host/x.bin").check().is_ok());
    }

    #[test]
    fn an_id_and_a_directory_are_required_too() {
        assert!(Job::new("", "/tmp", "https://h/x.bin").check().is_err());
        assert!(Job::new("m", "", "https://h/x.bin").check().is_err());
    }
}
