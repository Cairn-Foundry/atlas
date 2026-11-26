---
name: deep-review
description: Deep code review of a whole piece of work, not a single merge request. Takes any mix of sources in the prompt - GitLab MR or GitHub PR URLs, a branch of a local or remote repository, a local path, a commit range, a ticket or spec - reviews each one in depth (whole files at the head, callers, siblings, failure paths, tests) and then reviews them together - contracts between services, deploy order, pieces of the work missing from one side. Every finding is verified against a calibration list of what experienced reviewers reject before it is reported. Produces a consolidated report and, on request, posts inline comments and summaries on the MRs and PRs.
when_to_use: Use when the user asks for a deep, thorough or cross-project review, or a review of a change that spans several repositories or services (microservices, back and front, library and consumer), or names several MRs, PRs or branches to review together. For a single quick MR pass, a lighter review is enough.
disable-model-invocation: false
argument-hint: "<sources...> [post] [approve] [focus: ...]"
allowed-tools: Read, Grep, Glob, Bash, Agent
effort: high
---

Sources and options given by the user: **$ARGUMENTS**

You review a piece of work the way the best human reviewers do: you read the code around the diff, you follow the change into every service it touches, you prove each problem with a concrete scenario, and you stay silent on what good reviewers have learned not to flag. A finding you cannot trigger with a named input, state or interleaving is not a finding.

The unit of review is the **work**, not the merge request. In a multi-service system one feature is spread over several repositories; most of the costly bugs live between them - a field produced and never read, a validation relaxed on one side only, a consumer deployed before its producer. A review that looks at one MR at a time cannot see them. You can.

## Step 0 - Load the references

Paths are relative to `${CLAUDE_SKILL_DIR}`. Read the five always-load references before anything else, in this order:

| # | File | Holds |
|---|---|---|
| 1 | [sources.md](references/sources.md) | how to resolve and materialise each kind of source |
| 2 | [review-method.md](references/review-method.md) | the checks, the core of the review |
| 3 | [cross-service.md](references/cross-service.md) | the review of the sources together |
| 4 | [calibration.md](references/calibration.md) | the verification protocol and what not to flag |
| 5 | [comment-style.md](references/comment-style.md) | how to write findings, and the report template |

Then, per source, the stack reference that matches what you find. A stack with no reference is reviewed with the generic method.

| Stack | Reference |
|---|---|
| Laravel / PHP API | [stacks/laravel.md](references/stacks/laravel.md) |
| NestJS, TypeORM | [stacks/nestjs.md](references/stacks/nestjs.md) |
| Vue 3 | [stacks/vue.md](references/stacks/vue.md) |
| React | [stacks/react.md](references/stacks/react.md) |
| Svelte 5 / SvelteKit | [stacks/svelte.md](references/stacks/svelte.md) |
| Java, Spring Boot | [stacks/java.md](references/stacks/java.md) |
| Rust | [stacks/rust.md](references/stacks/rust.md) |
| Tauri v2 | [stacks/tauri.md](references/stacks/tauri.md) |
| PostgreSQL, SQL migrations | [stacks/postgresql.md](references/stacks/postgresql.md) |

Read [publishing.md](references/publishing.md) only if the user asked to post or approve.

A deep security pass (exploit proof, an auth or payment surface) is not this skill's job: route it to the `mythos` agent and say so in the report. The access-scope check of the method stays.

## Step 1 - Resolve the sources

Parse `$ARGUMENTS` into a list of sources following `sources.md`. For each one, establish:

- kind (MR, PR, branch, path, range, ticket or spec), repository, local clone path;
- head ref and base ref, and the diff between them;
- for an MR or PR: title, description, target branch, draft state, existing threads.

Options: `post` (publish findings on the MRs and PRs), `approve` (approve the ones left clean, only with `post`), `focus: ...` (weight the review toward an area, never skip the verification).

If a source cannot be resolved (no access, ambiguous name, no clone and no remote), say which one and why, and continue with the others. Ask the user only when **no** source resolves.

Print the resolved list before reviewing:

```
Sources
1. <repo> <kind> <head> vs <base> - <n files, +a -d>
2. ...
```

## Step 2 - Understand the work

- Restate in two or three sentences what the work as a whole is meant to do, from the titles, descriptions, tickets and commits. Name the role of each source in it (producer, consumer, migration, UI, shared library).
- Read each repository's own rules: `CLAUDE.md`, `AGENTS.md`, `CONTRIBUTING.md`, lint and formatter config, `.coderabbit.yaml` or other review bot config, PR templates. House rules come from there, not from you.
- Note each target branch. A release or hotfix branch means bugs and contract issues only, no refactor findings.

## Step 3 - Review each source

For each source, per `review-method.md` and its stack reference:

- Read whole files at the head, never a hunk alone.
- Grep every caller and sibling of every symbol the diff adds, removes or changes, in the whole repository at the head.
- Record the source's **contract surface** as defined in `cross-service.md`: what it produces, what it consumes, what it requires at deploy time.

Parallelism: when the total diff is under about 400 changed lines, do it yourself. Otherwise launch one `minos` agent per source, all in a single message; `minos` loads the method and the calibration itself and returns a fixed format, so the brief only carries what it cannot know:

- the source from Step 1: repository path, head and base refs, target branch;
- the work summary from Step 2, including its purpose (feature, fix, refactor, performance, migration): the purpose selects passes of the method, such as the unnecessary-work pass of a performance change;
- the scope: the whole source, or the files of its slice;
- the `focus` if any.

The brief adds no instruction that narrows the method. "Look for regressions" is not a scope: a reviewer told to compare with the base files everything left unchanged as verified.

A source over about 1500 changed lines gets several `minos` agents, split by area (a group of features, the shared infrastructure, the build and delivery), never by pass: each agent must run every check on its files.

Read every `VERIFIED` and `DROPPED` line the agents return, not only their findings: an observation filed there ("every error maps to X", "same params as before") can be a finding the agent did not see.

## Step 4 - Review the sources together

Run `cross-service.md` yourself, on the contract surfaces of all sources: match every produced item with its consumers, every consumed item with its producer, and derive the deploy order. Read the other side's code whenever a match looks wrong; if the other side is a repository not given as a source but reachable (local clone, remote), read it there and say so.

This step also runs with a single source: its contract surface still has consumers or producers somewhere, and a change to one side with nothing on the other is itself a finding to check.

## Step 5 - Verify every candidate

Apply the protocol of `calibration.md` to every candidate, yours and the agents'. Drop anything that fails a check. A finding that depends on a fact you could not verify becomes a question, never an assertion. Merge duplicates: the same problem in several places is one finding listing all the sites.

## Step 6 - Report

Print the report in the terminal, in the user's language, following the report template of `comment-style.md`: the work and its sources, verdict, cross-service findings first, then findings per source by severity, what was verified, deployment notes, and what was dropped at verification.

If `post` was given, publish per `publishing.md`: inline comments on the MR or PR that holds the line, one summary note per MR or PR, cross-service findings on every MR or PR they involve with a link to the other side. Then approve per `publishing.md` if `approve` was given. Without `post`, publish nothing.

## Step 7 - Clean up

Delete the refs created under `refs/review/` and any temporary clone, per `sources.md`.
