use std::path::{Path, PathBuf};

use super::{
    Backend, BackendResult, BranchEntry, FileStatus, LogEntry, Process, RevisionEntry, RevisionInfo, StashEntry, StatusInfo,
    TagEntry,
};

//use crate::tool;

pub struct Git;

impl Git {
    pub fn try_new() -> Option<(PathBuf, Self)> {
        let output = Process::spawn("git", &["rev-parse", "--show-toplevel"]).ok()?.wait().ok()?;

        let root = Path::new(output.trim()).into();
        Some((root, Self))
    }

    fn remote(&self) -> BackendResult<String> {
        let remote = Process::spawn("git", &["remote"])?.wait()?.trim().to_owned();
        Ok(remote)
    }

    fn current_branch(&self) -> BackendResult<String> {
        let branch = Process::spawn("git", &["symbolic-ref", "--short", "HEAD"])?.wait()?.trim().to_owned();
        Ok(branch)
    }

    fn remote_branch(&self) -> BackendResult<String> {
        let mut remote = self.remote()?;
        let current_branch = self.current_branch()?;
        remote.push_str("/");
        remote.push_str(&current_branch);
        Ok(remote)
    }
}

impl Backend for Git {
    fn status(&self) -> BackendResult<StatusInfo> {
        let output = Process::spawn("git", &["status", "--branch", "--no-rename", "--null"])?.wait()?;
        let mut splits = output.split('\0').map(str::trim);

        let header = splits.next().unwrap_or("").into();
        let entries = splits
            .filter(|e| e.len() >= 2)
            .map(|e| {
                let (status, filename) = e.split_at(2);
                RevisionEntry::new(filename.trim().into(), parse_file_status(status))
            })
            .collect();

        Ok(StatusInfo { header, entries })
    }

    fn commit(&self, message: &str, entries: &[RevisionEntry], amend: bool) -> BackendResult<()> {
        if entries.is_empty() {
            Process::spawn("git", &["add", "--all"])?.wait()?;
        } else {
            let mut args = vec!["add", "--"];
            for entry in entries {
                args.push(&entry.name);
            }

            Process::spawn("git", &args)?.wait()?;
        }

        if amend {
            Process::spawn("git", &["commit", "--amend", "--no-edit"])?.wait()?;
        } else {
            Process::spawn("git", &["commit", "-m", message])?.wait()?;
        }
        Ok(())
    }

    fn discard(&self, entries: &[RevisionEntry]) -> BackendResult<()> {
        if entries.is_empty() {
            Process::spawn("git", &["reset", "--hard", "HEAD"])?.wait()?;
            Process::spawn("git", &["clean", "--force"])?.wait()?;
        } else {
            let drop_entry = |f: fn(&FileStatus) -> bool, args: &[&str]| -> BackendResult<()> {
                let filter_entries: Vec<_> = entries.iter().filter(|&e| f(&e.status)).map(|e| e.name.as_str()).collect();

                if !filter_entries.is_empty() {
                    let args = [args.to_vec(), filter_entries].concat();
                    Process::spawn("git", &args)?.wait()?;
                }

                Ok(())
            };

            drop_entry(|status| matches!(status, FileStatus::Untracked), &["clean", "--force", "--"])?;
            drop_entry(|status| matches!(status, FileStatus::Added), &["rm", "--force", "--"])?;
            drop_entry(|status| !matches!(status, FileStatus::Untracked | FileStatus::Added), &["checkout", "HEAD", "--"])?;
        }

        Ok(())
    }

    fn diff(&self, revision: Option<&str>, entries: &[RevisionEntry]) -> BackendResult<String> {
        match revision {
            Some(revision) => {
                let parent = format!("{}~", revision);
                if entries.is_empty() {
                    Process::spawn("git", &["diff", &parent, revision])?.wait()
                } else {
                    let mut args = vec!["diff", &parent, revision, "--"];
                    for entry in entries {
                        args.push(&entry.name);
                    }

                    Process::spawn("git", &args)?.wait()
                }
            }
            None => {
                if entries.is_empty() {
                    Process::spawn("git", &["diff", "-z"])?.wait()
                } else {
                    let mut args = vec!["diff", "--"];
                    for entry in entries {
                        args.push(&entry.name);
                    }
                    Process::spawn("git", &args)?.wait()
                }
            }
        }
    }

    fn resolve_taking_ours(&self, entries: &[RevisionEntry]) -> BackendResult<()> {
        if entries.is_empty() {
            Process::spawn("git", &["checkout", "--ours", "."])?.wait()?;
        } else {
            if !entries.iter().any(|e| matches!(e.status, FileStatus::Unmerged)) {
                return Ok(());
            }

            let mut args = vec!["checkout", "--ours", "--"];
            for entry in entries {
                if let FileStatus::Unmerged = entry.status {
                    args.push(&entry.name);
                }
            }

            Process::spawn("git", &args)?.wait()?;
        }

        Ok(())
    }

    fn resolve_taking_theirs(&self, entries: &[RevisionEntry]) -> BackendResult<()> {
        if entries.is_empty() {
            Process::spawn("git", &["checkout", "--theirs", "."])?.wait()?;
        } else {
            if !entries.iter().any(|e| matches!(e.status, FileStatus::Unmerged)) {
                return Ok(());
            }

            let mut args = vec!["checkout", "--theirs", "--"];
            for entry in entries {
                if let FileStatus::Unmerged = entry.status {
                    args.push(&entry.name);
                }
            }

            Process::spawn("git", &args)?.wait()?;
        }

        Ok(())
    }

    fn log(&self, skip: usize, len: usize) -> BackendResult<(usize, Vec<LogEntry>)> {
        let skip_text = skip.to_string();
        let len = len.to_string();
        let template = "--format=format:%x00%h%x00%as%x00%aN%x00%D%x00%s";
        let output = Process::spawn(
            "git",
            &[
                "log",
                //"--all",
                "--decorate",
                "--oneline",
                "--graph",
                "--skip",
                &skip_text,
                "--max-count",
                &len,
                template,
            ],
        )?
        .wait()?;

        let mut entries = Vec::new();
        for line in output.lines() {
            let mut splits = line.splitn(6, '\0');

            let graph = splits.next().unwrap_or("").into();
            let hash = splits.next().unwrap_or("").into();
            let date = splits.next().unwrap_or("").into();
            let author = splits.next().unwrap_or("").into();
            let refs = splits.next().unwrap_or("").into();
            let message = splits.next().unwrap_or("").into();

            entries.push(LogEntry { graph, hash, date, author, refs, message });
        }

        Ok((skip, entries))
    }

    fn checkout(&self, revision: &str) -> BackendResult<()> {
        Process::spawn("git", &["checkout", revision])?.wait()?;
        Ok(())
    }

    fn merge(&self, revision: &str) -> BackendResult<()> {
        Process::spawn("git", &["merge", "--no-ff", revision])?.wait()?;
        Ok(())
    }

    fn fetch(&self) -> BackendResult<()> {
        Process::spawn("git", &["fetch", "--all", "--prune"])?.wait()?;
        Ok(())
    }

    fn pull(&self) -> BackendResult<()> {
        Process::spawn("git", &["pull", "--all"])?.wait()?;
        Ok(())
    }

    fn push(&self) -> BackendResult<()> {
        Process::spawn("git", &["push"])?.wait()?;
        Ok(())
    }

    fn push_gerrit(&self) -> BackendResult<()> {
        let remote = self.remote()?;
        let current_branch = self.current_branch()?;
        let mut branch_info = "HEAD:refs/for/".to_owned();
        branch_info.push_str(&current_branch);
        Process::spawn("git", &["push", &remote, &branch_info])?.wait()?;
        Ok(())
    }

    fn stash(&self, message: &str, entries: &[RevisionEntry]) -> BackendResult<()> {
        if entries.is_empty() {
            Process::spawn("git", &["stash", "save", message])?.wait()?;
        } else {
            let mut args =
                if message.is_empty() { vec!["stash", "push", "--"] } else { vec!["stash", "push", "-m", message, "--"] };
            for entry in entries {
                args.push(&entry.name);
            }

            Process::spawn("git", &args)?.wait()?;
        }

        Ok(())
    }

    fn stash_list(&self) -> BackendResult<Vec<StashEntry>> {
        let entries = Process::spawn("git", &["stash", "list"])?
            .wait()?
            .lines()
            .map(|l| {
                let mut splits = l.splitn(3, ':');
                let id = splits.next().unwrap().trim_matches(|c: char| !c.is_numeric()).parse::<usize>().unwrap();
                let branch = splits.next().unwrap().split(' ').next_back().unwrap().trim().to_owned();
                let message = splits.next().unwrap_or("").trim().to_owned();

                StashEntry { id, branch, message }
            })
            .collect();
        Ok(entries)
    }

    fn stash_pop(&self, id: usize) -> BackendResult<()> {
        Process::spawn("git", &["stash", "pop", id.to_string().as_str()])?.wait()?;
        Ok(())
    }

    fn stash_show(&self, id: usize) -> BackendResult<String> {
        Process::spawn("git", &["stash", "show", id.to_string().as_str()])?.wait()
    }

    fn stash_diff(&self, id: usize) -> BackendResult<String> {
        Process::spawn("git", &["stash", "show", "-p", id.to_string().as_str()])?.wait()
    }

    fn stash_drop(&self, id: usize) -> BackendResult<()> {
        Process::spawn("git", &["stash", "drop", id.to_string().as_str()])?.wait()?;
        Ok(())
    }

    fn reset(&self, revision: &str) -> BackendResult<()> {
        let output = Process::spawn("git", &["status", "--null"])?.wait()?;
        if !output.is_empty() {
            return Err("There are local changes! Please stash / commit / discard first.".to_owned());
        }
        let revision = if revision == "" { self.remote_branch()? } else { revision.to_owned() };
        Process::spawn("git", &["reset", "--hard", &revision])?.wait()?;
        Ok(())
    }

    fn revision_details(&self, revision: &str) -> BackendResult<RevisionInfo> {
        let message = Process::spawn("git", &["show", "-s", "--format=%B", "--no-renames", revision])?;
        let changes = Process::spawn("git", &["diff-tree", "--no-commit-id", "--name-status", "-r", "-z", revision])?;

        let message = message.wait()?.trim().into();

        let changes = changes.wait()?;
        let mut splits = changes.split('\0');

        let mut entries = Vec::new();
        loop {
            let status = match splits.next() {
                Some(status) if !status.is_empty() => parse_file_status(status),
                _ => break,
            };
            let name = match splits.next() {
                Some(name) => name.into(),
                None => break,
            };

            entries.push(RevisionEntry::new(name, status));
        }

        Ok(RevisionInfo { message, entries })
    }

    fn branches(&self) -> BackendResult<Vec<BranchEntry>> {
        let entries = Process::spawn(
            "git",
            &[
                "branch",
                "--list",
                //"--all",
                "--format=%(refname:short)%20%(HEAD)", // %20 is space, %(HEAD) is *
            ],
        )?
        .wait()?
        .lines()
        .map(|l| {
            let mut splits = l.splitn(2, ' ');
            let name = splits.next().unwrap_or("").into();
            let checked_out = splits.next().unwrap_or("") == "*";
            BranchEntry { name, checked_out }
        })
        .collect();
        Ok(entries)
    }

    fn new_branch(&self, name: &str) -> BackendResult<()> {
        //let remote = Process::spawn("git", &["remote"])?.wait()?;
        //Process::spawn("git", &["branch", name])?.wait()?;
        //Process::spawn("git", &["checkout", name])?.wait()?;
        //Process::spawn("git", &["push", "--set-upstream", remote.trim(), name])?.wait()?;
        Process::spawn("git", &["checkout", "-b", name])?.wait()?; // only local branch
        Ok(())
    }

    fn delete_branch(&self, name: &str, force: bool) -> BackendResult<()> {
        //let remote = Process::spawn("git", &["remote"])?.wait()?;
        let delete_option = if force { "-D" } else { "--delete" };
        Process::spawn("git", &["branch", delete_option, name])?.wait()?;
        //Process::spawn("git", &["push", "--delete", remote.trim(), name])?.wait()?;
        Ok(())
    }

    fn tags(&self) -> BackendResult<Vec<TagEntry>> {
        let entries = Process::spawn("git", &["tag", "--list", "--format=%(refname:short)"])?
            .wait()?
            .lines()
            .map(|l| TagEntry { name: l.into() })
            .collect();
        Ok(entries)
    }

    fn new_tag(&self, name: &str) -> BackendResult<()> {
        //let remote = Process::spawn("git", &["remote"])?.wait()?;
        Process::spawn("git", &["tag", "--force", name])?.wait()?;
        //Process::spawn("git", &["push", remote.trim(), name])?.wait()?;
        Ok(())
    }

    fn delete_tag(&self, name: &str) -> BackendResult<()> {
        //let remote = Process::spawn("git", &["remote"])?.wait()?;
        Process::spawn("git", &["tag", "--delete", name])?.wait()?;
        //Process::spawn("git", &["push", "--delete", remote.trim(), name])?.wait()?;
        Ok(())
    }
}

fn parse_file_status(s: &str) -> FileStatus {
    match s.chars().next() {
        Some('M') => FileStatus::Modified,
        Some('A') => FileStatus::Added,
        Some('D') => FileStatus::Deleted,
        Some('R') => FileStatus::Renamed,
        Some('?') => FileStatus::Untracked,
        Some('C') => FileStatus::Copied,
        Some('U') => FileStatus::Unmerged,
        Some(' ') => FileStatus::Clean,
        _ => FileStatus::Unknown(s.into()),
    }
}
