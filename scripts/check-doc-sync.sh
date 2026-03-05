#!/usr/bin/env bash
set -euo pipefail

STAGED_FILES="$(git diff --cached --name-only --diff-filter=ACMR)"

if [[ -z "${STAGED_FILES}" ]]; then
  exit 0
fi

is_doc_file() {
  local path="$1"
  case "${path}" in
    *.md|docs/*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

DOC_CHANGED=0
NON_DOC_CHANGED=0

while IFS= read -r file_path; do
  [[ -z "${file_path}" ]] && continue

  if is_doc_file "${file_path}"; then
    DOC_CHANGED=1
  else
    NON_DOC_CHANGED=1
  fi
done <<< "${STAGED_FILES}"

if [[ "${NON_DOC_CHANGED}" -eq 1 && "${DOC_CHANGED}" -eq 0 ]]; then
  cat <<'MSG'
Documentation sync check failed.

At least one in-repo documentation file (for example *.md or docs/*) must be staged whenever
non-documentation files are changed.

Update SPEC.md and/or related docs to match behavior and architecture changes, then re-stage.
MSG
  exit 1
fi

exit 0
