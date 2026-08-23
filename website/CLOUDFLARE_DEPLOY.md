# Cloudflare Workers deployment

Martty's website uses Cloudflare Workers Builds with the Cloudflare GitHub App.
Cloudflare owns the deploy credential; no Cloudflare token is stored in this
repository or in GitHub Actions secrets.

## One-time Git connection

1. In Cloudflare, open **Workers & Pages** and select the existing Worker named
   `deepseek-harness-tui`.
2. Open **Settings → Builds**, select **Connect**, and authorize the
   **Cloudflare Workers and Pages** GitHub App for `openma-ai/Martty`.
3. Configure the build:

   | Setting | Value |
   | --- | --- |
   | Repository | `openma-ai/Martty` |
   | Production branch | `main` |
   | Root directory | `website` |
   | Build command | `npm run ci` |
   | Deploy command | `npx wrangler deploy` |
   | Non-production deploy command | `npx wrangler versions upload` |

4. Enable **builds for non-production branches** so pull requests receive a
   versioned `workers.dev` preview URL and a GitHub check/comment.
5. Enable the build cache. Optionally limit build watch paths to `website/**`.
6. Save the connection. The first production build deploys `main` to the
   `martty.sh` Custom Domain declared in `wrangler.jsonc`.

The Worker name in the Cloudflare dashboard must remain identical to the
`name` in `wrangler.jsonc`. The Custom Domain requires `martty.sh` to be an
active zone in the same Cloudflare account and must not have a conflicting
CNAME record.

## Pipeline behavior

- Pull request: GitHub Actions runs tests, Astro checks, a production build,
  and a Wrangler dry run without credentials. Workers Builds uploads a preview
  version and posts its URL to the pull request.
- Push to `main`: the same checks run, then Workers Builds executes
  `wrangler deploy` and promotes the version serving `martty.sh`.
- Rollback: use the Worker's **Deployments** page to promote an earlier version.

## Local commands

```sh
cd website
npm ci
npm run ci
```

Manual deployment remains available to a maintainer already authenticated with
`wrangler login`, but it is not part of CI and no token should be committed.
