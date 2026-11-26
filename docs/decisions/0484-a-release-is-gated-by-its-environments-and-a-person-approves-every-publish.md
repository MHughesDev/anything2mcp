# 0484. A release is gated by its environments, and a person approves every publish

**Status:** Accepted. Design decision, made by the worker who ran the first design review round by lens 8, the security reviewer, under the lead's brief. The owner had not read this record when it was merged. It changes no public contract and no code; it changes what [publishing setup](../plan/publishing-setup.md) and [RELEASING.md](../../RELEASING.md) say the owner must set before the first publish, from "if a person should approve" to required.

**Date:** 2026-11-25

**Changes:** [publishing setup](../plan/publishing-setup.md), "How releases authenticate"; [RELEASING.md](../../RELEASING.md), "Publishing" and "The first release, 0.1.0"; [spec section 605](../spec/6-running-it/605-packaging-and-release.md) is unchanged, since it already asks for short-lived credentials and a gated pipeline.

## Context

The release workflow refuses to publish from a commit that is not on `main`, a version that is not the package's own, a version with no kept contract, and a changelog that is not ready. Those refusals are steps of `.github/workflows/release.yml` itself. Whoever can change that file on a branch can take them out, and three events run the file as the branch has it:

- a pull request from a branch of the repository that touches the release files, the workflow among them, runs the changed workflow. A job of a pull request from the same repository may ask for `id-token: write`, so it can be given a short-lived credential;
- a tag `v*` pushed on any commit runs the workflow as that commit has it;
- a start by hand runs the workflow of the branch it is started on.

PyPI's and npm's trusted publishers check the repository, the workflow's file name, and the environment's name. They do not check the branch or the commit. So the gate that a changed workflow cannot remove is the GitHub environment: its required reviewers and its rules for which branches and tags may deploy to it. The documents called both optional ("with required reviewers if a person should approve"), and a job that names an environment that does not exist creates it with no protection. In this project, agents push branches and may be steered ([AGENTS.md](../../AGENTS.md)); with the environments unprotected, a steered agent with push access could publish a build to PyPI or npm, which cannot be undone.

## Decision

1. **The environments `release`, `pypi`, and `npm` are the release's gate, and they are set before the first publish.** Each has the owner as a required reviewer, so that a person approves every job that can write the release page or a registry. Each allows deployments from `main` and from tags that match `v*` only, so that a job of a pull request or of another branch waits on nothing it can pass.
2. **Only the owner creates a tag `v*`.** A tag ruleset on `v*` restricts who may create, move, or delete one, because a tag on a commit that is not on `main` runs that commit's workflow.
3. **The workflow's own refusals stay, as a second barrier.** They catch a mistake made in good faith, such as a tag on the wrong commit; they are not the gate against a changed workflow, and the documents no longer present them as one.
4. **Until the three settings are made, the release is not started.** RELEASING.md's first step for 0.1.0 says so.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Make the workflow check that it runs from `main` or a tag | The check lives in the file a branch can change |
| Leave the reviewers optional, as before | A publish cannot be undone, and the rule that a person approves what cannot be taken back (rule 3's `irreversible`) applies to the product's own release as much as to a write |
| Run the release workflow only on tags and starts by hand, dropping its pull-request trigger | The rehearsal on every pull request that touches the release files is what proves the set before a publish; and a tag on another commit is the same hole |
| Check the environments' settings from CI | A token that reads environment settings is wider than CI needs, and the check would also live in a file a branch can change |

## Proof

None by test: the gate is a setting of the repository. The documents now name the three settings as required (publishing setup, RELEASING.md step 1 of the first release), and the security review report records the finding ([report](../reviews/security-review-2026-11-25.md), finding SR-1).

## Revisit if

The registries' trusted publishers can be bound to a branch or a tag; then the environments' branch rules become a second barrier too, and the reviewers remain.
