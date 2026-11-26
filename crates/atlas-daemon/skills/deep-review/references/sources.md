# Sources - resolving and materialising what to review

The user names sources in free form. Recognise each one, find or fetch the code, and pin a
**head** and a **base**. Never modify the user's working trees: no checkout, no reset, no stash.
Everything is read through refs (`git show <ref>:<path>`, `git grep <pattern> <ref>`).

## Contents

- Recognising sources
- Finding a local clone
- Pinning head and base
- The diff
- Access problems
- Cleanup

## Recognising sources

| The user writes | Kind | Resolution |
|---|---|---|
| `https://<gitlab-host>/<group>/<project>/-/merge_requests/<iid>` | GitLab MR | glab |
| `https://github.com/<owner>/<repo>/pull/<n>` | GitHub PR | gh |
| `!<iid>` or `#<n>` next to a project name | MR or PR | as above, project resolved by name |
| `<path>@<branch>`, `<repo-url>@<branch>`, "the `<branch>` branch of `<project>`" | branch | git |
| `<path>` alone, "my current changes" | local work | working tree vs base |
| `<sha1>..<sha2>` with a repo | range | git |
| a ticket key, a spec path, pasted text | context | read only, not diffed |

Ambiguous project names: look for a matching clone first (below); if several match, list them and
pick the one whose remote matches the name exactly, otherwise ask.

## Finding a local clone

Before cloning anything, look for an existing clone:

1. The current directory, its parent, and the siblings of the current repository.
2. A remote match: for each candidate directory, `git -C <dir> remote get-url origin` compared to
   the MR, PR or repo URL (ignore scheme, `.git` suffix and `git@host:` vs `https://host/`).

No clone found: make a partial clone in the session scratchpad (or a temporary directory):

```bash
git clone -q --filter=blob:none --no-checkout <remote-url> <scratch>/<repo>
```

Record it for cleanup.

## Pinning head and base

GitLab MR:

```bash
glab api "projects/<url-encoded-path>/merge_requests/<iid>"            # title, description, source/target, diff_refs, draft
glab api --paginate "projects/<url-encoded-path>/merge_requests/<iid>/discussions?per_page=100"
git -C <clone> fetch -q origin "+refs/merge-requests/<iid>/head:refs/review/<iid>"
git -C <clone> fetch -q origin "<target_branch>"
```

Head is `refs/review/<iid>` (check it equals `diff_refs.head_sha`); base is
`diff_refs.base_sha`. Keep `base_sha`, `start_sha` and `head_sha` if you may publish.

GitHub PR:

```bash
gh pr view <n> -R <owner>/<repo> --json title,body,baseRefName,headRefName,headRefOid,isDraft,url
gh api --paginate "repos/<owner>/<repo>/pulls/<n>/comments"
git -C <clone> fetch -q origin "+refs/pull/<n>/head:refs/review/pr-<n>" "<baseRefName>"
```

Base is `git merge-base origin/<baseRefName> refs/review/pr-<n>`.

Branch:

```bash
git -C <clone> fetch -q origin "+refs/heads/<branch>:refs/review/<branch-slug>"
```

Base is the branch the user named, else the default branch
(`git -C <clone> symbolic-ref --short refs/remotes/origin/HEAD`), taken as
`git merge-base origin/<default> refs/review/<branch-slug>`. If the branch is local only, use it
directly as head. If an open MR or PR exists for that branch, say so and use its target as base.

Local work: head is the working tree (`git -C <path> diff <base>` plus untracked files listed by
`git status --porcelain`); base as for a branch. Read files from disk.

Range: head and base are the two ends.

## The diff

```bash
git -C <clone> diff --stat <base> <head>
git -C <clone> diff <base> <head> -- <path>
```

Review files one by one for large diffs; never truncate silently. Skip generated files (lockfiles,
compiled assets, generated clients) except to check that their change matches a real dependency or
source change.

## Access problems

- glab or gh missing or unauthenticated: say which command failed and suggest the user runs
  `glab auth login` or `gh auth login` (prefixed with `!` in Claude Code to run it in-session).
  Continue with the sources that resolve.
- MR or PR refs not fetchable (fork, permissions): fetch the source branch from the source project
  instead.

## Cleanup

```bash
git -C <clone> for-each-ref --format='%(refname)' refs/review/ | xargs -r -n1 git -C <clone> update-ref -d
```

Delete temporary clones you created. Never delete a clone you found.
