#!/bin/bash
# push-tree.sh: the worktree's core/, renderer/ and native/ (tracked and new files, as on disk) into the
# guest's C:\s4\repo, replacing it. Needs WINVM_SSH, WINVM_WORK and WINVM_LOCK_LINE (see winvm.sh). Run from the repository root.
set -euo pipefail
: "${WINVM_SSH:?}" "${WINVM_WORK:?}" "${WINVM_LOCK_LINE:?}"
line=$(printf '%q' "$WINVM_LOCK_LINE")
ssh "$WINVM_SSH" "[ \"\$(cat '$WINVM_WORK/../winvm/LOCK' 2>/dev/null)\" = $line ]" \
  || { echo "push-tree.sh: the VM's LOCK does not hold WINVM_LOCK_LINE; nothing sent" >&2; exit 3; }
git ls-files -co --exclude-standard core renderer native | COPYFILE_DISABLE=1 tar --no-xattrs -cf - -T - | ssh "$WINVM_SSH" "cat > '$WINVM_WORK/src/repo.tar'"
ssh "$WINVM_SSH" "export WINVM_LOCK_LINE=$line && cd '$WINVM_WORK/../winvm/scripts' && ./copy.sh '$WINVM_WORK/src/repo.tar' g:C:/s4/repo.tar && ./ssh.sh 'if (Test-Path C:\\s4\\repo) { Remove-Item -Recurse -Force C:\\s4\\repo }; New-Item -ItemType Directory C:\\s4\\repo | Out-Null; tar -xf C:\\s4\\repo.tar -C C:\\s4\\repo; Remove-Item C:\\s4\\repo.tar' && rm -f '$WINVM_WORK/src/repo.tar'"
