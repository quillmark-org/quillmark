#!/bin/bash
set -e
set -o pipefail

# Strips the release-prepare seed coverage comment (`<!-- seed: … -->`, the
# raw commit list release-prepare.yml appends below a curated changelog
# section) from FILE, in place. Non-greedy and DOTALL so each block ends at
# its own `-->`, and multiple blocks strip independently. Surrounding blank
# lines collapse to the file's usual single-blank-line separator. A no-op
# when FILE holds no such block.

file="$1"
perl -0777 -pi -e 's/\n*<!-- seed:.*?-->\n*/\n\n/gs' "$file"
