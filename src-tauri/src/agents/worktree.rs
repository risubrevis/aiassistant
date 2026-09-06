//! Git-worktree isolation for parallel sub-agents (docs/16): each Write run
// happens in its own worktree on a temp branch; Approve merges it back,
// Reject discards it. All git access goes through the `git` subprocess.

use serde::Serialize;
use std::path::Path;

/// Handle to an isolated git worktree for one agent run (docs/16).
#[derive(Debug, Clone, Serialize)]
pub struct WorktreeHandle {
    /// Absolute path of the worktree directory.
    pub path: String,
    /// Branch name created for the worktree.
    pub branch: String,
}

pub fn is_git_repo(path: &str) -> bool {
    Path::new(path).join(".git").exists()
}

pub async fn create(
    base_repo: &str,
    base_ref: &str,
    name_hint: &str,
) -> anyhow::Result<WorktreeHandle> {
    if !is_git_repo(base_repo) {
        anyhow::bail!("not a git repository: {base_repo}");
    }
    let hint = sanitize_hint(name_hint)?;
    let branch = unique_branch(base_repo, &hint).await?;
    let path = unique_worktree_path(base_repo, &hint);
    let parent = Path::new(&path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("worktree path has no parent: {path}"))?
        .to_path_buf();
    tokio::fs::create_dir_all(&parent).await?;

    let base = resolve_base(base_repo, base_ref).await;
    let add = [
        "worktree",
        "add",
        "-b",
        branch.as_str(),
        path.as_str(),
        base.as_str(),
    ];
    if let Err(e) = run_git(base_repo, &add).await {
        // Branch may have been created between our uniqueness check and
        // `worktree add`; retry without `-b` so the worktree still lands.
        if !e.to_string().contains("already exists") {
            return Err(e);
        }
        run_git(
            base_repo,
            &["worktree", "add", path.as_str(), base.as_str()],
        )
        .await?;
    }
    Ok(WorktreeHandle { path, branch })
}

/// Symlink heavy gitignored dirs (node_modules, target, ...) from the base
/// repo into the worktree so dependencies don't need reinstalling (docs/16).
/// Partial failures are tolerated as long as at least one link succeeded.
pub async fn symlink_gitignored(worktree: &str, base_repo: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let base = std::fs::canonicalize(base_repo)
            .unwrap_or_else(|_| std::path::PathBuf::from(base_repo));
        let mut failures: Vec<String> = Vec::new();
        let mut linked = 0usize;
        for name in SYMLINK_DIRS {
            let target = base.join(name);
            let link = Path::new(worktree).join(name);
            if !target.is_dir() || link.exists() {
                continue;
            }
            match symlink(&target, &link) {
                Ok(()) => linked += 1,
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }
        if failures.is_empty() {
            return Ok(());
        }
        let detail = failures.join("; ");
        if linked == 0 {
            anyhow::bail!("gitignored symlinks all failed: {detail}");
        }
        tracing::warn!("gitignored symlinks partially failed ({linked} ok): {detail}");
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = (worktree, base_repo);
        Ok(())
    }
}

pub async fn merge_into_base(handle: &WorktreeHandle) -> anyhow::Result<()> {
    let base = base_repo_from_handle(handle)?;
    let out = tokio::process::Command::new("git")
        .args(["-C", &base, "merge", "--no-edit", &handle.branch])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to spawn git merge: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let combined = format!("{stdout}\n{stderr}");
        if combined.contains("CONFLICT") || combined.contains("Automatic merge failed") {
            let files = conflict_files(&combined);
            let listing = if files.is_empty() {
                "conflicted files (see git output)".to_string()
            } else {
                files.join(", ")
            };
            anyhow::bail!("merge conflict: {listing}");
        }
        anyhow::bail!("git merge {} failed: {}", handle.branch, combined.trim());
    }
    // Cleanup is best-effort: the merge already succeeded, so failures here
    // must not surface as errors. Worktree removal goes first — a branch
    // checked out in it cannot be deleted before its worktree is gone.
    if let Err(e) = run_git(&base, &["worktree", "remove", "--force", &handle.path]).await {
        tracing::warn!("worktree cleanup after merge failed: {e}");
    }
    if let Err(e) = run_git(&base, &["branch", "-d", &handle.branch]).await {
        tracing::warn!("worktree branch cleanup after merge failed: {e}");
    }
    Ok(())
}

pub async fn remove(handle: &WorktreeHandle) -> anyhow::Result<()> {
    let base = base_repo_from_handle(handle)?;
    if !is_git_repo(&base) {
        // Base repository is gone: the worktree dir is a plain leftover.
        if Path::new(&handle.path).exists() {
            tokio::fs::remove_dir_all(&handle.path).await?;
        }
        return Ok(());
    }
    if let Err(e) = run_git(&base, &["worktree", "remove", "--force", &handle.path]).await {
        // Already-removed or stale git metadata is not fatal; make sure the
        // directory itself does not linger.
        tracing::warn!("worktree remove failed (continuing): {e}");
        if Path::new(&handle.path).exists() {
            tokio::fs::remove_dir_all(&handle.path).await?;
        }
    }
    match run_git(&base, &["branch", "-D", handle.branch.as_str()]).await {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("not found") => {
            tracing::warn!("worktree branch already gone: {}", handle.branch);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Per-file entry of a live diff.
#[derive(Debug, Clone, Serialize)]
pub struct WorktreeFileDiff {
    pub path: String,
    /// "added" | "modified" | "deleted" | "renamed"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub patch: String,
}

/// Live diff of everything an agent changed in its run directory.
#[derive(Debug, Clone, Serialize, Default)]
pub struct WorktreeDiff {
    pub files: Vec<WorktreeFileDiff>,
    pub stat: String,
    pub total_additions: u32,
    pub total_deletions: u32,
}

/// Live diff of an isolated worktree vs the branch point (merge-base of the
/// base repo HEAD and the worktree HEAD). Includes committed, uncommitted and
/// untracked changes — everything the agent changed. Used by in-app Approve/Reject.
pub async fn diff(handle: &WorktreeHandle) -> anyhow::Result<WorktreeDiff> {
    let base = base_repo_from_handle(handle)?;
    let base_head = run_git(&base, &["rev-parse", "HEAD"]).await?;
    let merge_base = run_git(&handle.path, &["merge-base", &base_head, "HEAD"])
        .await
        .unwrap_or(base_head);
    diff_against(&handle.path, &merge_base).await
}

/// Diff a working directory against its HEAD (shared-cwd runs without a
/// worktree branch). Returns empty diff if `dir` is not a git repo.
pub async fn diff_against_head(dir: &str) -> anyhow::Result<WorktreeDiff> {
    if !is_git_repo(dir) {
        return Ok(WorktreeDiff::default());
    }
    let head = run_git(dir, &["rev-parse", "HEAD"]).await?;
    diff_against(dir, &head).await
}

const MAX_PATCH_CHARS: usize = 200_000;

async fn diff_against(dir: &str, base_commit: &str) -> anyhow::Result<WorktreeDiff> {
    let name_status = run_git(dir, &["diff", "--name-status", base_commit]).await?;
    let mut files = Vec::new();
    for line in name_status.lines().filter(|l| !l.trim().is_empty()) {
        let Some((code, rest)) = line.trim().split_once('\t') else {
            continue;
        };
        let status = if code.starts_with('R') {
            "renamed"
        } else if code.starts_with('D') {
            "deleted"
        } else if code.starts_with('A') {
            "added"
        } else {
            "modified"
        };
        let path = if code.starts_with('R') {
            rest.rsplit('\t').next().unwrap_or_default()
        } else {
            rest
        };
        if path.is_empty() {
            continue;
        }
        let numstat = run_git(dir, &["diff", "--numstat", base_commit, "--", path]).await?;
        let (additions, deletions) = numstat
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| {
                let mut cols = l.split('\t');
                (parse_count(cols.next()), parse_count(cols.next()))
            })
            .unwrap_or((0, 0));
        let patch = clamp(run_git(dir, &["diff", base_commit, "--", path]).await?);
        files.push(WorktreeFileDiff {
            path: path.to_string(),
            status: status.into(),
            additions,
            deletions,
            patch,
        });
    }

    let mut untracked_stats = Vec::new();
    let untracked = run_git(dir, &["ls-files", "--others", "--exclude-standard"]).await?;
    for path in untracked.lines().map(str::trim).filter(|p| !p.is_empty()) {
        let full = Path::new(dir).join(path);
        if !full.is_file() {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&full).await else {
            continue;
        };
        let content = clamp(String::from_utf8_lossy(&bytes).into_owned());
        let additions = content.lines().count() as u32;
        let mut patch = format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
        );
        for line in content.lines() {
            patch.push('+');
            patch.push_str(line);
            patch.push('\n');
        }
        untracked_stats.push((path.to_string(), additions));
        files.push(WorktreeFileDiff {
            path: path.to_string(),
            status: "added".into(),
            additions,
            deletions: 0,
            patch,
        });
    }

    let mut stat = run_git(dir, &["diff", "--stat", base_commit]).await?;
    if !untracked_stats.is_empty() {
        let extra: Vec<String> = untracked_stats
            .iter()
            .map(|(path, additions)| format!(" {path} | {additions} +"))
            .collect();
        if !stat.is_empty() {
            stat.push('\n');
        }
        stat.push_str(&extra.join("\n"));
    }

    let total_additions = files.iter().map(|f| f.additions).sum();
    let total_deletions = files.iter().map(|f| f.deletions).sum();
    Ok(WorktreeDiff {
        files,
        stat,
        total_additions,
        total_deletions,
    })
}

fn parse_count(v: Option<&str>) -> u32 {
    v.unwrap_or("-").parse().unwrap_or(0)
}

fn clamp(s: String) -> String {
    if s.chars().count() <= MAX_PATCH_CHARS {
        return s;
    }
    let mut cut: String = s.chars().take(MAX_PATCH_CHARS).collect();
    cut.push_str("\n... (truncated)");
    cut
}

#[cfg(unix)]
const SYMLINK_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "build",
    "dist",
    "__pycache__",
    ".next",
    "vendor",
];

const WORKTREE_ROOT: &str = ".aiassistant-worktrees";
const BRANCH_PREFIX: &str = "aiagent";

async fn run_git(repo: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("git")
        .args(["-C", repo])
        .args(args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to spawn git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        anyhow::bail!("git {} failed: {detail}", args.join(" "));
    }
    Ok(stdout)
}

async fn ref_exists(repo: &str, refname: &str) -> bool {
    let out = tokio::process::Command::new("git")
        .args(["-C", repo, "rev-parse", "--verify", "--quiet", refname])
        .output()
        .await;
    matches!(out, Ok(o) if o.status.success())
}

fn sanitize_hint(hint: &str) -> anyhow::Result<String> {
    let cleaned: String = hint
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        anyhow::bail!("invalid worktree name hint: {hint:?}");
    }
    Ok(cleaned)
}

fn unique_worktree_path(base_repo: &str, hint: &str) -> String {
    let root = Path::new(base_repo).join(WORKTREE_ROOT);
    let mut candidate = root.join(hint);
    let mut n = 1u32;
    while candidate.exists() {
        n += 1;
        candidate = root.join(format!("{hint}-{n}"));
    }
    candidate.to_string_lossy().into_owned()
}

async fn unique_branch(repo: &str, hint: &str) -> anyhow::Result<String> {
    let base = format!("{BRANCH_PREFIX}/{hint}");
    if !ref_exists(repo, &format!("refs/heads/{base}")).await {
        return Ok(base);
    }
    for n in 2..1000u32 {
        let candidate = format!("{base}-{n}");
        if !ref_exists(repo, &format!("refs/heads/{candidate}")).await {
            return Ok(candidate);
        }
    }
    anyhow::bail!("cannot find unique worktree branch for hint {hint:?}")
}

/// Resolve the commit-ish a worktree branch starts from.
/// "fresh" → origin's default branch, falling back to "head" behavior (local
/// HEAD) when the repo has no usable remote.
async fn resolve_base(repo: &str, base_ref: &str) -> String {
    if base_ref != "fresh" {
        return "HEAD".to_string();
    }
    if let Ok(symref) = run_git(repo, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await {
        let r = symref.trim().to_string();
        if !r.is_empty() && ref_exists(repo, &r).await {
            return r;
        }
    }
    if let Ok(show) = run_git(repo, &["remote", "show", "origin"]).await {
        for line in show.lines() {
            if let Some(idx) = line.find("HEAD branch:") {
                let name = line[idx + "HEAD branch:".len()..].trim();
                if !name.is_empty() {
                    let candidate = format!("refs/remotes/origin/{name}");
                    if ref_exists(repo, &candidate).await {
                        return candidate;
                    }
                }
                break;
            }
        }
    }
    tracing::warn!("worktree base_ref=fresh: no remote default branch in {repo}, using HEAD");
    "HEAD".to_string()
}

fn base_repo_from_handle(handle: &WorktreeHandle) -> anyhow::Result<String> {
    let base = Path::new(&handle.path)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "cannot derive base repository from worktree path: {}",
                handle.path
            )
        })?;
    Ok(base.to_string_lossy().into_owned())
}

fn conflict_files(output: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("CONFLICT") else {
            continue;
        };
        let Some((_, detail)) = rest.split_once("): ") else {
            continue;
        };
        let detail = detail.strip_prefix("Merge conflict in ").unwrap_or(detail);
        if let Some(file) = detail.split_whitespace().next() {
            if !files.iter().any(|f| f == file) {
                files.push(file.to_string());
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempRepo {
        path: String,
    }

    impl TempRepo {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ia-wt-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let path = dir.to_string_lossy().into_owned();
            git_ok(&path, &["init"]);
            git_ok(&path, &["config", "user.email", "t@t"]);
            git_ok(&path, &["config", "user.name", "t"]);
            git_ok(&path, &["config", "commit.gpgsign", "false"]);
            std::fs::write(dir.join("README.md"), "# test\n").expect("write readme");
            git_ok(&path, &["add", "."]);
            git_ok(&path, &["commit", "-m", "init"]);
            Self { path }
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn git(repo: &str, args: &[&str]) -> std::process::Output {
        std::process::Command::new("git")
            .args(["-C", repo])
            .args(args)
            .output()
            .expect("spawn git")
    }

    fn git_ok(repo: &str, args: &[&str]) -> String {
        let out = git(repo, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn branch_exists(repo: &str, branch: &str) -> bool {
        let spec = format!("refs/heads/{branch}");
        git(repo, &["rev-parse", "--verify", "--quiet", spec.as_str()])
            .status
            .success()
    }

    #[tokio::test]
    async fn create_adds_worktree() {
        let repo = TempRepo::new();
        let handle = create(&repo.path, "head", "create-test")
            .await
            .expect("create worktree");
        assert_eq!(handle.branch, "aiagent/create-test");
        let wt = PathBuf::from(&handle.path);
        assert!(wt.exists(), "worktree dir must exist");
        assert!(wt.join(".git").exists(), "worktree must contain .git");
        assert!(
            branch_exists(&repo.path, &handle.branch),
            "branch must exist in the base repo"
        );
    }

    #[tokio::test]
    async fn merge_applies_changes() {
        let repo = TempRepo::new();
        let handle = create(&repo.path, "head", "merge-test")
            .await
            .expect("create worktree");
        std::fs::write(Path::new(&handle.path).join("agent.txt"), "from agent")
            .expect("write file in worktree");
        git_ok(&handle.path, &["add", "."]);
        git_ok(&handle.path, &["commit", "-m", "agent change"]);

        merge_into_base(&handle).await.expect("merge must succeed");

        let merged = repo.path.clone();
        let file = std::path::Path::new(&merged).join("agent.txt");
        assert!(file.exists(), "merged file must appear in base repo");
        assert_eq!(
            std::fs::read_to_string(&file).expect("read merged file"),
            "from agent"
        );
        assert!(
            !branch_exists(&repo.path, &handle.branch),
            "worktree branch must be deleted after merge"
        );
        assert!(
            !Path::new(&handle.path).exists(),
            "worktree dir must be removed after merge"
        );
    }

    #[tokio::test]
    async fn remove_discards() {
        let repo = TempRepo::new();
        let handle = create(&repo.path, "head", "remove-test")
            .await
            .expect("create worktree");
        std::fs::write(Path::new(&handle.path).join("junk.txt"), "junk")
            .expect("write untracked file");

        remove(&handle).await.expect("remove must succeed");

        assert!(
            !Path::new(&handle.path).exists(),
            "worktree dir must be gone"
        );
        assert!(
            !branch_exists(&repo.path, &handle.branch),
            "worktree branch must be deleted on remove"
        );
    }

    #[tokio::test]
    async fn is_git_repo_false_for_plain_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ia-wt-{}-plain-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create plain temp dir");
        let path = dir.to_string_lossy().into_owned();
        assert!(!is_git_repo(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn diff_shows_live_worktree_changes() {
        let repo = TempRepo::new();
        std::fs::write(Path::new(&repo.path).join("extra.txt"), "extra line\n")
            .expect("write extra file");
        git_ok(&repo.path, &["add", "."]);
        git_ok(&repo.path, &["commit", "-m", "extra"]);

        let handle = create(&repo.path, "head", "diff-test")
            .await
            .expect("create worktree");

        let wt = Path::new(&handle.path);
        std::fs::write(wt.join("README.md"), "# test\nedited by agent\n").expect("edit tracked");
        std::fs::write(wt.join("notes.txt"), "a\nb\nc\n").expect("write untracked");
        std::fs::remove_file(wt.join("extra.txt")).expect("delete tracked file");

        let d = diff(&handle).await.expect("live diff");

        let readme = d
            .files
            .iter()
            .find(|f| f.path == "README.md")
            .expect("modified file in diff");
        assert_eq!(readme.status, "modified");
        let deleted = d
            .files
            .iter()
            .find(|f| f.path == "extra.txt")
            .expect("deleted file in diff");
        assert_eq!(deleted.status, "deleted");
        let added = d
            .files
            .iter()
            .find(|f| f.path == "notes.txt")
            .expect("untracked file in diff");
        assert_eq!(added.status, "added");
        assert!(added.patch.contains("+++ b/notes.txt"));
        assert!(added.patch.contains("+a\n"));
        assert!(d.total_additions > 0);
        assert!(d.total_deletions > 0);
    }

    #[tokio::test]
    async fn diff_against_head_shows_uncommitted() {
        let repo = TempRepo::new();
        std::fs::write(
            Path::new(&repo.path).join("README.md"),
            "# test\nnew line\n",
        )
        .expect("edit readme");
        std::fs::write(Path::new(&repo.path).join("todo.txt"), "one\ntwo\n").expect("untracked");

        let d = diff_against_head(&repo.path).await.expect("head diff");

        let readme = d
            .files
            .iter()
            .find(|f| f.path == "README.md")
            .expect("modified file in diff");
        assert_eq!(readme.status, "modified");
        let added = d
            .files
            .iter()
            .find(|f| f.path == "todo.txt")
            .expect("untracked file in diff");
        assert_eq!(added.status, "added");
        assert_eq!(added.additions, 2);
        assert_eq!(added.deletions, 0);
        assert!(d.total_additions >= 3);
    }

    #[tokio::test]
    async fn diff_against_head_empty_for_non_git_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ia-wt-{}-plain-diff-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create plain temp dir");
        let path = dir.to_string_lossy().into_owned();
        std::fs::write(Path::new(&path).join("x.txt"), "content\n").expect("write file");

        let d = diff_against_head(&path)
            .await
            .expect("non-git dir must yield empty diff, not an error");

        assert!(d.files.is_empty());
        assert_eq!(d.stat, "");
        assert_eq!(d.total_additions, 0);
        assert_eq!(d.total_deletions, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_gitignored_links_dependency_dirs() {
        let repo = TempRepo::new();
        let root = std::path::Path::new(&repo.path).to_path_buf();
        std::fs::create_dir_all(root.join("node_modules")).expect("create node_modules");
        std::fs::write(root.join("node_modules").join("probe"), "shared dep")
            .expect("write probe file");

        let handle = create(&repo.path, "head", "symlink-test")
            .await
            .expect("create worktree");
        symlink_gitignored(&handle.path, &repo.path)
            .await
            .expect("symlink must succeed");

        let linked = std::path::Path::new(&handle.path).join("node_modules");
        assert!(
            linked.is_symlink(),
            "node_modules must be a symlink on unix"
        );
        assert_eq!(
            std::fs::read_to_string(linked.join("probe")).expect("read through symlink"),
            "shared dep"
        );

        let _ = std::fs::remove_dir_all(&repo.path);
    }
}
