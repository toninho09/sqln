#!/bin/sh
# Build the documentation site. Copies the public documents into book/src,
# redirects the links to internal documents to the repository, and runs mdbook.
# Run "make docs" or "make docs-serve" from the repository root.

set -eu

public_docs="
README.md
docs/ARCHITECTURE.md
docs/FEATURE-MAP.md
docs/KEYMAP.md
docs/MCP.md
docs/RESULTS.md
docs/TOOLS.md
docs/UI.md
"

# Documents that stay out of the site. Their links point to the repository.
internal='AI_COMPLETION|DECISIONS|INITIAL_PLAN|PAGINATION-VALIDATION'
blob='https://github.com/toninho09/sqln/blob/main'

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/book/src"

mkdir -p "$src/docs/images"
cp "$root/docs/images/"*.png "$src/docs/images/"

for file in $public_docs; do
  sed -E "s#\]\\((docs/)?($internal)\\.md#]($blob/docs/\\2.md#g" \
    "$root/$file" > "$src/$file"
done

mdbook build "$root/book"
