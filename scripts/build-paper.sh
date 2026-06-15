#!/usr/bin/env bash
# Build the-membrane-complete.md and PDF from whitepaper + appendix.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOCS="$ROOT/docs"
OUT_MD="$DOCS/the-membrane-complete.md"
OUT_PDF="$DOCS/the-membrane-complete.pdf"

python3 - "$DOCS" "$OUT_MD" << 'PY'
import sys
from pathlib import Path

docs = Path(sys.argv[1])
out = Path(sys.argv[2])
wp = (docs / "whitepaper.md").read_text()
ap = (docs / "appendix-open-research.md").read_text()

replacements = [
    (
        "- [Appendix B: Open Research & Prototype Stack](appendix-open-research.md)",
        "- [Appendix B: Open Research & Prototype Stack](#appendix-b-open-research--prototype-stack)",
    ),
    (
        "Implement the MVP (§9) using the stacks in [Appendix B](./appendix-open-research.md).",
        "Implement the MVP (§9) using the stacks in [Appendix B](#appendix-b-open-research--prototype-stack).",
    ),
    (
        "[^21]: NeuroLM, SYNAPTICON, Brain-LLM Interface — see [appendix-open-research.md](./appendix-open-research.md).",
        "[^21]: NeuroLM, SYNAPTICON, Brain-LLM Interface — see [Appendix B](#appendix-b-open-research--prototype-stack).",
    ),
]
for old, new in replacements:
    wp = wp.replace(old, new)

ap_body = ap.split("\n", 1)[1] if ap.startswith("# Appendix B:") else ap
lines = ap_body.splitlines()
while lines and (
    lines[0].strip() == ""
    or "Companion to" in lines[0]
    or "Independent research" in lines[0]
    or lines[0].strip().startswith("IAC and")
):
    lines.pop(0)
ap_body = "\n".join(lines).lstrip()

appendix_block = "\n\n---\n\n# Appendix B: Open Research & Prototype Stack\n\n" + ap_body
marker = "# How to Contribute"
if marker not in wp:
    raise SystemExit("Could not find How to Contribute section in whitepaper.md")
complete = wp.replace(marker, appendix_block + "\n\n" + marker)
out.write_text(complete)
print(f"Wrote {out} ({len(complete.splitlines())} lines)")
PY

if command -v pandoc >/dev/null 2>&1; then
  export DYLD_FALLBACK_LIBRARY_PATH="/opt/homebrew/lib:${DYLD_FALLBACK_LIBRARY_PATH:-}"
  export PATH="${HOME}/Library/Python/3.9/bin:${PATH}"
  if command -v weasyprint >/dev/null 2>&1 || python3 -c "import weasyprint" 2>/dev/null; then
    pandoc "$OUT_MD" -o "$OUT_PDF" \
      --pdf-engine=weasyprint \
      -V papersize=a4 \
      -V author="Zorie R. Barber" \
      --toc --toc-depth=3 \
      -f markdown+footnotes
    echo "Wrote $OUT_PDF"
  else
    echo "No weasyprint PDF engine. Markdown only: $OUT_MD"
  fi
else
  echo "pandoc not found. Markdown only: $OUT_MD"
fi
