# Releasing Martty

Every change to `main` — releases included — has an issue and a pull request.
Nobody pushes to `main` directly, admin bypass of the `main` rulesets included.
(The 0.3.3-beta.1 and 0.3.3 release commits `ba38698` and `2b1b07a` were pushed
directly; #174 is their retroactive record. History was not rewritten.)

## Steps

1. **Release issue.** Open `Release X.Y.Z` describing what ships: merged PRs,
   bundled dependency versions (e.g. `@openma/deepseek-harness-acp`), and the
   verification plan. Move `CHANGELOG.md` `[Unreleased]` into `[X.Y.Z]` in a
   normal PR (or in the release PR below).
2. **Release PR.** From a clean, up-to-date `main`:

   ```sh
   node scripts/release.mjs X.Y.Z          # or X.Y.Z-beta.N
   git push -u origin release/vX.Y.Z
   gh pr create --title "release: vX.Y.Z" --body "Closes #<release issue>"
   ```

   `release.mjs` creates `release/vX.Y.Z` and commits the version bump of
   `npm/package.json`, `npm/package-lock.json`, `Cargo.toml` and `Cargo.lock`
   there. CI (including the required `dsh plugin (latest)` / `dsh plugin (next)`
   checks and the release check) must pass; merge it normally with a squash merge.
3. **Tag the merge.** On the updated `main`:

   ```sh
   git switch main && git pull
   node scripts/release.mjs X.Y.Z --tag
   git push origin vX.Y.Z
   ```

   `--tag` refuses unless the merged versions equal `X.Y.Z`. The tag push runs
   `package-npm.yml`: `release-tag` checks versions and notes, `publish`
   publishes to npm (`-alpha.`/`-beta.`/`-rc.` tags get the `beta` dist-tag,
   everything else `latest`), and `release` creates the GitHub Release with
   generated notes.
4. **Verify and record.** Verify the published package (dsh plugin install +
   boot on dsh `latest` and `next`, `npx martty@X.Y.Z --dsh <profile>`,
   `npm i -g`) and post the results on the release issue.
