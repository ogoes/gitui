//! merging from upstream

use super::BranchType;
use crate::{
    error::{Error, Result},
    sync::{merge_msg, utils, CommitId},
};
use git2::Commit;
use scopetime::scope_time;

/// merge upstream using a merge commit if we did not create conflicts.
/// if we did not create conflicts we create a merge commit and return the commit id.
/// Otherwise we return `None`
pub fn merge_upstream_commit(
    repo_path: &str,
    branch_name: &str,
) -> Result<Option<CommitId>> {
    scope_time!("merge_upstream_commit");

    let repo = utils::repo(repo_path)?;

    let branch = repo.find_branch(branch_name, BranchType::Local)?;
    let upstream = branch.upstream()?;

    let upstream_commit = upstream.get().peel_to_commit()?;

    let annotated_upstream = repo
        .reference_to_annotated_commit(&upstream.into_reference())?;

    let (analysis, pref) =
        repo.merge_analysis(&[&annotated_upstream])?;

    if !analysis.is_normal() {
        return Err(Error::Generic(
            "normal merge not possible".into(),
        ));
    }

    if analysis.is_fast_forward() && pref.is_fastforward_only() {
        return Err(Error::Generic(
            "ff merge would be possible".into(),
        ));
    }

    //TODO: support merge on unborn?
    if analysis.is_unborn() {
        return Err(Error::Generic("head is unborn".into()));
    }

    repo.merge(&[&annotated_upstream], None, None)?;

    if !repo.index()?.has_conflicts() {
        let msg = merge_msg(repo_path)?;

        let commit_id =
            commit_merge_with_head(&repo, &[upstream_commit], &msg)?;

        return Ok(Some(commit_id));
    }

    Ok(None)
}

pub(crate) fn commit_merge_with_head(
    repo: &git2::Repository,
    commits: &[Commit],
    msg: &str,
) -> Result<CommitId> {
    let signature =
        crate::sync::commit::signature_allow_undefined_name(repo)?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let head_commit = repo.find_commit(
        crate::sync::utils::get_head_repo(repo)?.into(),
    )?;

    let mut parents = vec![&head_commit];
    parents.extend(commits);

    let commit_id = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            msg,
            &tree,
            parents.as_slice(),
        )?
        .into();
    repo.cleanup_state()?;
    Ok(commit_id)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::sync::{
        branch_compare_upstream,
        remotes::{fetch, push::push},
        tests::{
            debug_cmd_print, get_commit_ids, repo_clone,
            repo_init_bare, write_commit_file,
        },
        RepoState,
    };

    #[test]
    fn test_merge_normal() {
        let (r1_dir, _repo) = repo_init_bare().unwrap();

        let (clone1_dir, clone1) =
            repo_clone(r1_dir.path().to_str().unwrap()).unwrap();

        let (clone2_dir, clone2) =
            repo_clone(r1_dir.path().to_str().unwrap()).unwrap();

        let clone2_dir = clone2_dir.path().to_str().unwrap();

        // clone1

        let commit1 =
            write_commit_file(&clone1, "test.txt", "test", "commit1");

        push(
            clone1_dir.path().to_str().unwrap(),
            "origin",
            "master",
            false,
            None,
            None,
        )
        .unwrap();

        // clone2

        let commit2 = write_commit_file(
            &clone2,
            "test2.txt",
            "test",
            "commit2",
        );

        //push should fail since origin diverged
        assert!(push(
            clone2_dir, "origin", "master", false, None, None,
        )
        .is_err());

        //lets fetch from origin
        let bytes = fetch(clone2_dir, "master", None, None).unwrap();
        assert!(bytes > 0);

        //we should be one commit behind
        assert_eq!(
            branch_compare_upstream(clone2_dir, "master")
                .unwrap()
                .behind,
            1
        );

        let merge_commit =
            merge_upstream_commit(clone2_dir, "master")
                .unwrap()
                .unwrap();

        let state = crate::sync::repo_state(clone2_dir).unwrap();
        assert_eq!(state, RepoState::Clean);

        assert!(!clone2.head_detached().unwrap());

        let commits = get_commit_ids(&clone2, 10);
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0], merge_commit);
        assert_eq!(commits[1], commit2);
        assert_eq!(commits[2], commit1);

        //verify commit msg
        let details =
            crate::sync::get_commit_details(clone2_dir, merge_commit)
                .unwrap();
        assert_eq!(
            details.message.unwrap().combine(),
            String::from("Merge remote-tracking branch 'refs/remotes/origin/master'")
        );
    }

    #[test]
    fn test_merge_normal_non_ff() {
        let (r1_dir, _repo) = repo_init_bare().unwrap();

        let (clone1_dir, clone1) =
            repo_clone(r1_dir.path().to_str().unwrap()).unwrap();

        let (clone2_dir, clone2) =
            repo_clone(r1_dir.path().to_str().unwrap()).unwrap();

        // clone1

        write_commit_file(
            &clone1,
            "test.bin",
            "test\nfooo",
            "commit1",
        );

        debug_cmd_print(
            clone2_dir.path().to_str().unwrap(),
            "git status",
        );

        push(
            clone1_dir.path().to_str().unwrap(),
            "origin",
            "master",
            false,
            None,
            None,
        )
        .unwrap();

        // clone2

        write_commit_file(
            &clone2,
            "test.bin",
            "foobar\ntest",
            "commit2",
        );

        let bytes = fetch(
            clone2_dir.path().to_str().unwrap(),
            "master",
            None,
            None,
        )
        .unwrap();
        assert!(bytes > 0);

        let res = merge_upstream_commit(
            clone2_dir.path().to_str().unwrap(),
            "master",
        )
        .unwrap();

        //this should not have commited cause we left conflicts behind
        assert_eq!(res, None);

        let state = crate::sync::repo_state(
            clone2_dir.path().to_str().unwrap(),
        )
        .unwrap();

        //validate the repo is in a merge state now
        assert_eq!(state, RepoState::Merge);

        //check that we still only have the first commit
        let commits = get_commit_ids(&clone1, 10);
        assert_eq!(commits.len(), 1);
    }
}
