use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
};

use crate::mode::{fuzzy_matches, FilterEntry};

pub mod git;

pub type BackendResult<T> = std::result::Result<T, String>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Copied,
    Unmerged,
    _Missing,
    _Ignored,
    Clean,
    Unknown(String),
}
impl FileStatus {
    pub const fn max_len() -> usize {
        9
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Modified => "modified",
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Untracked => "untracked",
            Self::Copied => "copied",
            Self::Unmerged => "unmerged",
            Self::_Missing => "missing",
            Self::_Ignored => "ignored",
            Self::Clean => "clean",
            Self::Unknown(status) => {
                if status.len() > Self::max_len() {
                    &status[..Self::max_len()]
                } else {
                    status
                }
            }
        }
    }
}

pub struct StatusInfo {
    pub header: String,
    pub entries: Vec<RevisionEntry>,
}

pub struct RevisionInfo {
    pub message: String,
    pub entries: Vec<RevisionEntry>,
}

#[derive(Clone, Debug)]
pub struct RevisionEntry {
    pub selected: bool,
    pub name: String,
    pub status: FileStatus,
}
impl RevisionEntry {
    pub fn new(name: String, status: FileStatus) -> Self {
        Self { selected: false, name, status }
    }
}
impl FilterEntry for RevisionEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool {
        fuzzy_matches(&self.name, pattern)
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub graph: String,
    pub hash: String,
    pub date: String,
    pub author: String,
    pub refs: String,
    pub message: String,
}
impl FilterEntry for LogEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool {
        fuzzy_matches(&self.message, pattern)
            || fuzzy_matches(&self.refs, pattern)
            || fuzzy_matches(&self.author, pattern)
            || fuzzy_matches(&self.date, pattern)
            || fuzzy_matches(&self.hash, pattern)
    }
}

#[derive(Clone, Debug)]
pub struct BranchEntry {
    pub name: String,
    pub checked_out: bool,
}
impl FilterEntry for BranchEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool {
        fuzzy_matches(&self.name, pattern)
    }
}

#[derive(Clone, Debug)]
pub struct TagEntry {
    pub name: String,
}
impl FilterEntry for TagEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool {
        fuzzy_matches(&self.name, pattern)
    }
}

#[derive(Clone, Debug)]
pub struct StashEntry {
    pub id: usize,
    pub branch: String,
    pub message: String,
}
impl FilterEntry for StashEntry {
    fn fuzzy_matches(&self, pattern: &str) -> bool {
        fuzzy_matches(&self.branch, pattern) || fuzzy_matches(&self.message, pattern)
    }
}

pub trait Backend: 'static + Send + Sync {
    fn status(&self) -> BackendResult<StatusInfo>;
    fn commit(&self, message: &str, entries: &[RevisionEntry], amend: bool) -> BackendResult<()>;
    fn discard(&self, entries: &[RevisionEntry]) -> BackendResult<()>;
    fn diff(&self, revision: Option<&str>, entries: &[RevisionEntry]) -> BackendResult<String>;
    fn resolve_taking_ours(&self, entries: &[RevisionEntry]) -> BackendResult<()>;
    fn resolve_taking_theirs(&self, entries: &[RevisionEntry]) -> BackendResult<()>;

    fn log(&self, start: usize, len: usize) -> BackendResult<(usize, Vec<LogEntry>)>;
    fn checkout(&self, revision: &str) -> BackendResult<()>;
    fn merge(&self, revision: &str) -> BackendResult<()>;
    fn fetch(&self) -> BackendResult<()>;
    fn pull(&self) -> BackendResult<()>;
    fn push(&self) -> BackendResult<()>;
    fn push_gerrit(&self) -> BackendResult<()>;
    fn reset(&self, revision: &str) -> BackendResult<()>;

    fn stash(&self, message: &str, entries: &[RevisionEntry]) -> BackendResult<()>;
    fn stash_list(&self) -> BackendResult<Vec<StashEntry>>;
    fn stash_pop(&self, id: usize) -> BackendResult<()>;
    fn stash_show(&self, id: usize) -> BackendResult<String>;
    fn stash_diff(&self, id: usize) -> BackendResult<String>;
    fn stash_drop(&self, id: usize) -> BackendResult<()>;

    fn revision_details(&self, revision: &str) -> BackendResult<RevisionInfo>;

    fn branches(&self) -> BackendResult<Vec<BranchEntry>>;
    fn new_branch(&self, name: &str) -> BackendResult<()>;
    fn delete_branch(&self, name: &str, force: bool) -> BackendResult<()>;

    fn tags(&self) -> BackendResult<Vec<TagEntry>>;
    fn new_tag(&self, name: &str) -> BackendResult<()>;
    fn delete_tag(&self, name: &str) -> BackendResult<()>;
}

pub struct Process(Child);
impl Process {
    pub fn spawn(command_name: &str, args: &[&str]) -> BackendResult<Self> {
        let mut command = Command::new(command_name);
        command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        match command.spawn() {
            Ok(child) => Ok(Self(child)),
            Err(error) => Err(format!("could not spawn process '{}': {}", command_name, error)),
        }
    }

    pub fn wait(self) -> BackendResult<String> {
        let output = match self.0.wait_with_output() {
            Ok(output) => output,
            Err(error) => return Err(format!("could not wait for process: {}", error)),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.success() {
            Ok(stdout.into())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut error = String::new();
            error.push_str(&stdout);
            error.push('\n');
            error.push_str(&stderr);
            Err(error)
        }
    }
}

pub fn backend_from_current_repository() -> Option<(PathBuf, Arc<dyn Backend>)> {
    if let Some((root, git)) = git::Git::try_new() {
        Some((root, Arc::new(git)))
    } else {
        None
    }
}
