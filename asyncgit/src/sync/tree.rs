use super::{utils::bytes2string, CommitId};
use crate::{error::Result, sync::utils::repo};
use git2::{Oid, Repository, Tree};
use scopetime::scope_time;
use std::path::{Path, PathBuf};

/// `tree_files` returns a list of `FileTree`
#[derive(Debug, PartialEq)]
pub struct TreeFile {
    /// path of this file
    pub path: PathBuf,
    /// unix filemode
    pub filemode: i32,
    // internal object id
    id: Oid,
}

///
pub fn tree_files(
    repo_path: &str,
    commit: CommitId,
) -> Result<Vec<TreeFile>> {
    scope_time!("tree_files");

    let repo = repo(repo_path)?;

    let commit = repo.find_commit(commit.into())?;
    let tree = commit.tree()?;

    let mut files: Vec<TreeFile> = Vec::new();

    tree_recurse(&repo, &PathBuf::from("./"), &tree, &mut files)?;

    Ok(files)
}

///
pub fn tree_file_content(
    repo_path: &str,
    file: &TreeFile,
) -> Result<String> {
    scope_time!("tree_file_content");

    let repo = repo(repo_path)?;

    let blob = repo.find_blob(file.id)?;
    let content = String::from_utf8(blob.content().into())?;

    Ok(content)
}

///
fn tree_recurse(
    repo: &Repository,
    path: &Path,
    tree: &Tree,
    out: &mut Vec<TreeFile>,
) -> Result<()> {
    out.reserve(tree.len());

    for e in tree {
        let path = path.join(bytes2string(e.name_bytes())?);
        match e.kind() {
            Some(git2::ObjectType::Blob) => {
                let id = e.id();
                let filemode = e.filemode();
                out.push(TreeFile { path, filemode, id });
            }
            Some(git2::ObjectType::Tree) => {
                let obj = e.to_object(repo)?;
                let tree = obj.peel_to_tree()?;
                tree_recurse(repo, &path, &tree, out)?;
            }
            Some(_) | None => (),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::{repo_init, write_commit_file};
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn test_smoke() {
        let (_td, repo) = repo_init().unwrap();
        let root = repo.path().parent().unwrap();
        let repo_path = root.as_os_str().to_str().unwrap();

        let c1 =
            write_commit_file(&repo, "test.txt", "content", "c1");

        let files = tree_files(repo_path, c1).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("./test.txt"));

        let c2 =
            write_commit_file(&repo, "test.txt", "content2", "c2");

        let content =
            tree_file_content(repo_path, &files[0]).unwrap();
        assert_eq!(&content, "content");

        let files_c2 = tree_files(repo_path, c2).unwrap();

        assert_eq!(files_c2.len(), 1);
        assert_ne!(files_c2[0], files[0]);
    }
}
