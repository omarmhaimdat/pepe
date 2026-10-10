#!/usr/bin/env bash
# Upload the site to the R2 bucket behind pepe.mhaimdat.com: the install
# page, the documentation site, the recordings and images they show, and
# the JSON Schema of each report. Run by the release (publish-r2.yml) and
# on its own whenever any of it changes on master (publish-site.yml).
#
#   site/publish.sh REPO_ROOT
#
# Needs AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_DEFAULT_REGION,
# R2_BUCKET and R2_ENDPOINT in the environment, as the workflows set them.
set -euo pipefail
root="${1:-.}"
r2() { aws s3 cp --endpoint-url "$R2_ENDPOINT" --only-show-errors "$@"; }

# The install page and what it shows
r2 "$root/site/index.html" "s3://$R2_BUCKET/index.html" \
  --content-type "text/html; charset=utf-8" --cache-control "public, max-age=300"
r2 "$root/assets/logo.svg" "s3://$R2_BUCKET/assets/logo.svg" \
  --content-type "image/svg+xml" --cache-control "public, max-age=86400"
for png in "$root"/site/img/*.png; do
  [ -f "$png" ] || continue
  r2 "$png" "s3://$R2_BUCKET/img/$(basename "$png")" \
    --content-type "image/png" --cache-control "public, max-age=86400"
done

# The documentation site (site/docs/, built from docs/)
for page in "$root"/site/docs/*.html; do
  r2 "$page" "s3://$R2_BUCKET/docs/$(basename "$page")" \
    --content-type "text/html; charset=utf-8" --cache-control "public, max-age=300"
done
r2 "$root/site/docs/docs.css" "s3://$R2_BUCKET/docs/docs.css" \
  --content-type "text/css; charset=utf-8" --cache-control "public, max-age=300"
r2 "$root/site/docs/docs.js" "s3://$R2_BUCKET/docs/docs.js" \
  --content-type "text/javascript; charset=utf-8" --cache-control "public, max-age=300"
r2 "$root/site/docs/search.json" "s3://$R2_BUCKET/docs/search.json" \
  --content-type "application/json; charset=utf-8" --cache-control "public, max-age=300"

# The recordings and the card the pages show
r2 "$root/assets/compare-card.svg" "s3://$R2_BUCKET/assets/compare-card.svg" \
  --content-type "image/svg+xml" --cache-control "public, max-age=86400"
for gif in "$root"/assets/*.gif; do
  [ -f "$gif" ] || continue
  r2 "$gif" "s3://$R2_BUCKET/assets/$(basename "$gif")" \
    --content-type "image/gif" --cache-control "public, max-age=86400"
done

# What an agent reads first
r2 "$root/llms.txt" "s3://$R2_BUCKET/llms.txt" \
  --content-type "text/plain; charset=utf-8" --cache-control "public, max-age=300"

# The JSON Schema of each report, for scripts and agents
for schema in "$root"/schema/*.schema.json; do
  [ -f "$schema" ] || continue
  r2 "$schema" "s3://$R2_BUCKET/schema/$(basename "$schema")" \
    --content-type "application/schema+json; charset=utf-8" --cache-control "public, max-age=300"
done
