# Contributing

All pull requests must use the official `gh stack` extension, including changes with one layer. Do not create standalone PRs through the website, `gh pr create` or another tool. Use [GitHub stacked pull requests](https://docs.github.com/en/pull-requests/how-tos/create-pull-requests/creating-stacked-pull-requests) for related changes. Each layer must contain one working, verified change. Target the branch directly below it so reviewers see that layer's diff.

Start from a clean worktree. Keep unrelated edits in their own worktree.

```sh
git fetch origin
gh stack init --base main stack/example-core
```

Adopt existing branches with `gh stack init --base main existing/bottom existing/top`. Link existing PRs in dependency order with `gh stack link <bottom-pr> <top-pr>`, then manage their branches with `gh stack`. Keep unrelated changes in separate stacks.

Stage exact files with `git add -- path/to/file`, then commit the completed change. Add the next layer after its parent works.

```sh
gh stack add stack/example-ui
```

Publish verified layers as drafts, or mark the stack ready when every published layer is ready for review.

```sh
gh stack submit --auto
gh stack submit --auto --open
```

Put the parent PR and actual verification results in each description. Keep plans, credentials, local agent instructions and unfinished work private.

After changing a lower layer, update its dependents from a clean checkout.

```sh
gh stack rebase
gh stack push
gh stack view
```

Review and merge from the bottom with `gh stack merge`. `main` requires a PR and rejects force pushes and deletion. Never bypass its protection. GitHub applies the base branch's review and check rules across a native stack.
