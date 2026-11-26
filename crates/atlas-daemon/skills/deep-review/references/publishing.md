# Publishing findings (only with the `post` option)

Publishing is outward-facing: do it only when the user asked for `post`, and only on the MRs and
PRs among the sources. A branch or local source has nowhere to post; its findings stay in the
report.

Write every body to a JSON file and send it with `--input`: it keeps backticks, newlines and
suggestion blocks intact.

## GitLab

Inline comment, on a line added or modified by the MR (`new_line`), using the `diff_refs` fetched
in `sources.md`:

```bash
cat > <scratch>/c.json <<'EOF'
{
  "body": "**[HIGH]**\n\n<finding>",
  "position": {
    "position_type": "text",
    "base_sha": "<base_sha>",
    "start_sha": "<start_sha>",
    "head_sha": "<head_sha>",
    "new_path": "<path>",
    "old_path": "<old path, same as new_path unless renamed>",
    "new_line": <line>
  }
}
EOF
glab api -X POST "projects/<url-encoded-path>/merge_requests/<iid>/discussions" \
  -H "Content-Type: application/json" --input <scratch>/c.json
```

For an unchanged context line, set both `old_line` and `new_line`. A line not in the diff cannot be
anchored: put the finding in the summary note.

Summary note:

```bash
glab api -X POST "projects/<url-encoded-path>/merge_requests/<iid>/notes" \
  -H "Content-Type: application/json" --input <scratch>/note.json    # {"body": "..."}
```

Resolve a thread whose problem is fixed at the head:

```bash
glab api -X PUT "projects/<url-encoded-path>/merge_requests/<iid>/discussions/<id>" -f resolved=true
```

Approve:

```bash
glab api -X POST "projects/<url-encoded-path>/merge_requests/<iid>/approve"
```

## GitHub

Post one review holding every inline comment of the PR and the summary:

```bash
cat > <scratch>/review.json <<'EOF'
{
  "commit_id": "<headRefOid>",
  "event": "COMMENT",
  "body": "<summary note>",
  "comments": [
    { "path": "<path>", "line": <line>, "side": "RIGHT", "body": "**[HIGH]**\n\n<finding>" }
  ]
}
EOF
gh api -X POST "repos/<owner>/<repo>/pulls/<n>/reviews" --input <scratch>/review.json
```

`event` is `APPROVE` to approve, `REQUEST_CHANGES` when blocking findings exist and the user asked
for it. Resolving threads needs the GraphQL `resolveReviewThread` mutation.

## Rules

- One distinct problem per inline comment, on the MR or PR that holds the line.
- A cross-service finding goes on each MR or PR involved, each comment linking the other side.
- Never re-post what an existing thread already raises.
- Resolve only threads whose fix is clearly visible at the head.
- Approve only with `approve`, and only an MR or PR that is not a draft, received no CRITICAL, HIGH
  or MEDIUM finding (cross-service findings included), and has no open thread left unresolved.
- Report every posted comment and every approval in the terminal, with its URL.
