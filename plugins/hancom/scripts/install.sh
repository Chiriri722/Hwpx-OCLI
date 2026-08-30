#!/usr/bin/env bash
#
# OfficeCLI 플러그인 디스커버리 경로에 설치한다.
#
# 프로토콜 §3의 탐색 순서 중 2순위인 사용자 플러그인 디렉터리를 쓴다:
#   ~/.officecli/plugins/<kind>/<ext>/plugin
# <kind>는 kebab-case, <ext>는 점 없는 확장자.
#
# 사용법:
#   scripts/install.sh              # 릴리즈 빌드 후 설치
#   scripts/install.sh --no-build   # 이미 빌드된 바이너리로 설치
#   scripts/install.sh --uninstall  # 제거
#   scripts/install.sh --print-env  # 설치 없이 환경변수 방식 안내

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_NAMES=("officecli-hancom-hwp" "officecli-hancom-hwp" "officecli-hancom-hwpx" "officecli-hancom-hwpx")
BUILT_BINS=(
  "${REPO_ROOT}/target/release/${BIN_NAMES[0]}"
  "${REPO_ROOT}/target/release/${BIN_NAMES[1]}"
  "${REPO_ROOT}/target/release/${BIN_NAMES[2]}"
  "${REPO_ROOT}/target/release/${BIN_NAMES[3]}"
)
PLUGIN_NAMES=("officecli-hancom-hwp" "officecli-hancom-hwp" "officecli-hancom-hwpx" "officecli-hancom-hwpx")
KINDS=("dump-reader" "dump-reader" "format-handler" "format-handler")
EXTENSIONS=("hwp" "hml" "hwpx" "owpml")
ENV_VARS=(
  "OFFICECLI_PLUGIN_DUMP_READER_HWP"
  "OFFICECLI_PLUGIN_DUMP_READER_HML"
  "OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX"
  "OFFICECLI_PLUGIN_FORMAT_HANDLER_OWPML"
)
CANONICAL_INDEXES=(0 0 2 2)
LINK_TARGETS=("" "../hwp/plugin" "" "../hwpx/plugin")

# Releases before format-handler promotion installed these two dump-reader
# paths. They are part of the same rollback domain and must be absent after a
# successful install so they cannot shadow the editable handlers.
OBSOLETE_KINDS=("dump-reader" "dump-reader")
OBSOLETE_EXTENSIONS=("hwpx" "owpml")

DO_BUILD=1
ACTION="install"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build)   DO_BUILD=0 ;;
    --uninstall)  ACTION="uninstall" ;;
    --print-env)  ACTION="print-env" ;;
    -h|--help)
      sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 64  # EX_USAGE
      ;;
  esac
  shift
done

if [[ -z "${HOME-}" || "${HOME-}" != /* ]]; then
  echo "error: HOME must be an absolute path" >&2
  exit 64  # EX_USAGE
fi

OFFICECLI_DIR="${HOME}/.officecli"
PLUGINS_DIR="${OFFICECLI_DIR}/plugins"
PLUGIN_ROOTS=("${PLUGINS_DIR}/dump-reader" "${PLUGINS_DIR}/format-handler")

INSTALL_DIRS=()
INSTALL_PATHS=()
for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  INSTALL_DIRS+=("${PLUGINS_DIR}/${KINDS[$index]}/${EXTENSIONS[$index]}")
  INSTALL_PATHS+=("${PLUGINS_DIR}/${KINDS[$index]}/${EXTENSIONS[$index]}/plugin")
done

OBSOLETE_DIRS=()
OBSOLETE_PATHS=()
for ((index = 0; index < ${#OBSOLETE_EXTENSIONS[@]}; index++)); do
  OBSOLETE_DIRS+=("${PLUGINS_DIR}/${OBSOLETE_KINDS[$index]}/${OBSOLETE_EXTENSIONS[$index]}")
  OBSOLETE_PATHS+=("${PLUGINS_DIR}/${OBSOLETE_KINDS[$index]}/${OBSOLETE_EXTENSIONS[$index]}/plugin")
done

MANAGED_KINDS=("${KINDS[@]}" "${OBSOLETE_KINDS[@]}")
MANAGED_EXTENSIONS=("${EXTENSIONS[@]}" "${OBSOLETE_EXTENSIONS[@]}")
MANAGED_DIRS=("${INSTALL_DIRS[@]}" "${OBSOLETE_DIRS[@]}")
MANAGED_PATHS=("${INSTALL_PATHS[@]}" "${OBSOLETE_PATHS[@]}")

target_label() {
  local index="$1"
  printf '%s/%s\n' "${MANAGED_KINDS[$index]}" "${MANAGED_EXTENSIONS[$index]}"
}

assert_install_directories_not_links() {
  local dir
  for dir in \
    "${OFFICECLI_DIR}" \
    "${PLUGINS_DIR}" \
    "${PLUGIN_ROOTS[@]}" \
    "${MANAGED_DIRS[@]}"
  do
    if [[ -L "${dir}" ]]; then
      echo "error: refusing reparseable install directory: ${dir}" >&2
      return 1
    fi
    if [[ -e "${dir}" && ! -d "${dir}" ]]; then
      echo "error: refusing non-directory install path component: ${dir}" >&2
      return 1
    fi
  done
}

is_safe_expected_link() {
  local index="$1"
  local path="$2"
  local actual canonical_index canonical_path

  actual="$(readlink "${path}")" || return 1

  if (( index < ${#EXTENSIONS[@]} )) && [[ -n "${LINK_TARGETS[$index]}" ]]; then
    canonical_index="${CANONICAL_INDEXES[$index]}"
    canonical_path="${INSTALL_PATHS[$canonical_index]}"
    if [[ "${actual}" == "${LINK_TARGETS[$index]}" && -f "${canonical_path}" && ! -L "${canonical_path}" ]]; then
      return 0
    fi
  fi

  # Exact compatibility shape emitted by the pre-promotion installer:
  # dump-reader/{hwp,hml,owpml}/plugin -> ../hwpx/plugin.
  if [[ "${MANAGED_KINDS[$index]}" == "dump-reader" &&
        "${MANAGED_EXTENSIONS[$index]}" != "hwpx" &&
        "${actual}" == "../hwpx/plugin" ]]; then
    canonical_path="${PLUGINS_DIR}/dump-reader/hwpx/plugin"
    if [[ -f "${canonical_path}" && ! -L "${canonical_path}" ]]; then
      return 0
    fi
  fi

  return 1
}

assert_install_targets_safe() {
  local index path
  for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
    path="${MANAGED_PATHS[$index]}"
    if [[ -L "${path}" ]]; then
      if ! is_safe_expected_link "${index}" "${path}"; then
        echo "error: refusing unexpected or broken install target link: ${path}" >&2
        return 1
      fi
    elif [[ -e "${path}" && ! -f "${path}" ]]; then
      echo "error: refusing non-file install target: ${path}" >&2
      return 1
    fi
  done
}

case "${ACTION}" in
  uninstall)
    assert_install_directories_not_links || exit 73  # EX_CANTCREAT
    assert_install_targets_safe || exit 73  # EX_CANTCREAT

    # Remove aliases before either their new or legacy canonical file.
    UNINSTALL_ORDER=(5 3 1 0 2 4)
    for index in "${UNINSTALL_ORDER[@]}"; do
      path="${MANAGED_PATHS[$index]}"
      dir="${MANAGED_DIRS[$index]}"
      if [[ -e "${path}" || -L "${path}" ]]; then
        rm -f "${path}"
        echo "removed ${path}"
      else
        echo "not installed: ${path}"
      fi
      rmdir "${dir}" 2>/dev/null || true
    done
    exit 0
    ;;

  print-env)
    for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
      echo "export ${ENV_VARS[$index]}=\"${BUILT_BINS[$index]}\""
    done
    exit 0
    ;;
esac

if [[ "${DO_BUILD}" -eq 1 ]]; then
  echo "building release binaries..."
  if ! command -v cargo >/dev/null 2>&1; then
    if [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
      export PATH="${HOME}/.cargo/bin:${PATH}"
    else
      echo "error: cargo not found. install Rust (https://rustup.rs) first." >&2
      exit 69  # EX_UNAVAILABLE
    fi
  fi
  ( cd "${REPO_ROOT}" && cargo build --release --locked )
fi

for index in 0 2; do
  if [[ ! -x "${BUILT_BINS[$index]}" ]]; then
    echo "error: binary not found at ${BUILT_BINS[$index]}" >&2
    echo "run without --no-build, or run: cargo build --release" >&2
    exit 69
  fi
done

manifest_matches_target() {
  local path="$1"
  local index="$2"
  local manifest name_needle protocol_needle kind_needle extension_needle

  manifest="$("${path}" --info 2>/dev/null)" || return 1
  name_needle="\"name\":\"${PLUGIN_NAMES[$index]}\""
  protocol_needle="\"protocol\":1"
  kind_needle="\"${KINDS[$index]}\""
  extension_needle="\".${EXTENSIONS[$index]}\""
  [[ "${manifest}" == *"${name_needle}"* ]] || return 1
  [[ "${manifest}" == *"${protocol_needle}"* ]] || return 1
  [[ "${manifest}" == *"${kind_needle}"* ]] || return 1
  [[ "${manifest}" == *"${extension_needle}"* ]] || return 1
  if [[ "${KINDS[$index]}" == "dump-reader" ]]; then
    [[ "${manifest}" == *"\"target\":\"docx\""* ||
       "${manifest}" == *"\"target\":\"xlsx\""* ||
       "${manifest}" == *"\"target\":\"pptx\""* ]] || return 1
  fi
}

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if ! manifest_matches_target "${BUILT_BINS[$index]}" "${index}"; then
    echo "error: '${BIN_NAMES[$index]} --info' does not match ${KINDS[$index]}/${EXTENSIONS[$index]}; refusing to install." >&2
    exit 70  # EX_SOFTWARE
  fi
done

# Preflight every active and obsolete path before creating a stage or moving a
# recovery backup. This prevents a late unsafe path from causing partial work.
assert_install_directories_not_links || exit 73  # EX_CANTCREAT
assert_install_targets_safe || exit 73  # EX_CANTCREAT
for dir in "${INSTALL_DIRS[@]}"; do
  mkdir -p "${dir}"
done
assert_install_directories_not_links || exit 73  # EX_CANTCREAT
assert_install_targets_safe || exit 73  # EX_CANTCREAT
chmod go-w "${INSTALL_DIRS[@]}"

STAGED=()
BACKUPS=()
HAD_EXISTING=()
COMMITTED=()
for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  STAGED+=("")
done
for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
  BACKUPS+=("")
  HAD_EXISTING+=(0)
  COMMITTED+=(0)
done

cleanup_staging() {
  local index staged_path
  for ((index = 0; index < ${#STAGED[@]}; index++)); do
    staged_path="${STAGED[$index]}"
    if [[ -n "${staged_path}" ]] && ! rm -f "${staged_path}"; then
      echo "warning: could not remove staged ${KINDS[$index]}/${EXTENSIONS[$index]} target: ${staged_path}" >&2
    fi
  done
  return 0
}
trap cleanup_staging EXIT

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if [[ -z "${LINK_TARGETS[$index]}" ]]; then
    STAGED[$index]="$(mktemp "${INSTALL_DIRS[$index]}/.plugin.tmp.XXXXXX")"
    install -m 0755 "${BUILT_BINS[$index]}" "${STAGED[$index]}"
    if ! cmp -s "${BUILT_BINS[$index]}" "${STAGED[$index]}"; then
      echo "error: staged ${KINDS[$index]}/${EXTENSIONS[$index]} plugin checksum mismatch" >&2
      exit 70
    fi
    if ! manifest_matches_target "${STAGED[$index]}" "${index}"; then
      echo "error: staged ${KINDS[$index]}/${EXTENSIONS[$index]} plugin failed its manifest check" >&2
      exit 70
    fi
  else
    for _ in {1..32}; do
      candidate="${INSTALL_DIRS[$index]}/.plugin-link.$$.$RANDOM"
      STAGED[$index]="${candidate}"
      if ln -s "${LINK_TARGETS[$index]}" "${candidate}" 2>/dev/null; then
        break
      fi
      STAGED[$index]=""
    done
    if [[ -z "${STAGED[$index]}" ]]; then
      echo "error: could not stage the ${KINDS[$index]}/${EXTENSIONS[$index]} discovery link" >&2
      exit 73
    fi
  fi
done

assert_install_directories_not_links || exit 73  # EX_CANTCREAT
assert_install_targets_safe || exit 73  # EX_CANTCREAT

unique_backup() {
  local dir="$1" name="$2" candidate
  for _ in {1..32}; do
    candidate="${dir}/.${name}.backup.$$.$RANDOM"
    if [[ ! -e "${candidate}" && ! -L "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

active_target_matches() {
  local index="$1"
  local path="${INSTALL_PATHS[$index]}"
  if [[ -n "${LINK_TARGETS[$index]}" ]]; then
    [[ -L "${path}" && "$(readlink "${path}")" == "${LINK_TARGETS[$index]}" ]]
  else
    [[ -f "${path}" && ! -L "${path}" ]] && cmp -s "${path}" "${BUILT_BINS[$index]}"
  fi
}

rollback_install() {
  local index path backup label
  local -a unrecovered=()
  local rollback_incomplete=0
  for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
    unrecovered+=(0)
  done

  for ((index = ${#MANAGED_PATHS[@]} - 1; index >= 0; index--)); do
    path="${MANAGED_PATHS[$index]}"
    label="$(target_label "${index}")"
    if [[ "${COMMITTED[$index]}" -ne 1 ]]; then
      continue
    fi
    if (( index < ${#EXTENSIONS[@]} )); then
      if [[ -e "${path}" || -L "${path}" ]]; then
        if ! active_target_matches "${index}"; then
          echo "warning: committed ${label} target changed; refusing rollback removal: ${path}" >&2
          unrecovered[$index]=1
        elif ! rm -f "${path}"; then
          echo "warning: could not remove committed ${label} target during rollback: ${path}" >&2
          unrecovered[$index]=1
        fi
      fi
    elif [[ -e "${path}" || -L "${path}" ]]; then
      echo "warning: retired ${label} target reappeared; refusing to overwrite it during rollback: ${path}" >&2
      unrecovered[$index]=1
    fi
  done

  for ((index = ${#MANAGED_PATHS[@]} - 1; index >= 0; index--)); do
    if [[ "${HAD_EXISTING[$index]}" -ne 1 ]]; then
      continue
    fi
    path="${MANAGED_PATHS[$index]}"
    backup="${BACKUPS[$index]}"
    label="$(target_label "${index}")"
    if [[ "${unrecovered[$index]}" -ne 0 ]]; then
      continue
    fi
    if [[ -e "${path}" || -L "${path}" ]]; then
      echo "error: cannot restore ${label}; target exists and recovery backup was preserved: ${backup}" >&2
      unrecovered[$index]=1
    elif [[ -n "${backup}" && ( -e "${backup}" || -L "${backup}" ) ]]; then
      if mv "${backup}" "${path}"; then
        BACKUPS[$index]=""
      else
        echo "error: could not restore ${label} recovery backup: ${backup}" >&2
        unrecovered[$index]=1
      fi
    else
      echo "error: ${label} recovery backup is missing during rollback: ${backup}" >&2
      unrecovered[$index]=1
    fi
  done

  for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
    if [[ "${unrecovered[$index]}" -ne 0 ]]; then
      rollback_incomplete=1
    fi
  done
  if [[ "${rollback_incomplete}" -ne 0 ]]; then
    echo "error: rollback incomplete; recovery backups were preserved where possible" >&2
  fi
  return 0
}

cleanup_recovery_backup() {
  local label="$1"
  local path="$2"
  if [[ -n "${path}" ]] && ! rm -f "${path}"; then
    echo "warning: ${label} backup cleanup failed; recovery backup preserved at: ${path}" >&2
  fi
}

# Commit the four new paths first. The old dump-reader paths remain discoverable
# until both format-handler paths exist; only then are they retired.
for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
  path="${MANAGED_PATHS[$index]}"
  label="$(target_label "${index}")"
  if [[ -e "${path}" || -L "${path}" ]]; then
    BACKUPS[$index]="$(unique_backup "${MANAGED_DIRS[$index]}" plugin)" || {
      rollback_install
      exit 73
    }
    if ! mv "${path}" "${BACKUPS[$index]}"; then
      if [[ ( -e "${BACKUPS[$index]}" || -L "${BACKUPS[$index]}" ) &&
            ! -e "${path}" && ! -L "${path}" ]]; then
        HAD_EXISTING[$index]=1
      fi
      rollback_install
      exit 73
    fi
    HAD_EXISTING[$index]=1
  fi

  if (( index < ${#EXTENSIONS[@]} )); then
    if ! mv "${STAGED[$index]}" "${path}"; then
      if [[ ! -e "${STAGED[$index]}" && ! -L "${STAGED[$index]}" &&
            ( -e "${path}" || -L "${path}" ) ]]; then
        STAGED[$index]=""
        COMMITTED[$index]=1
      fi
      rollback_install
      exit 73
    fi
    STAGED[$index]=""
  fi
  COMMITTED[$index]=1
done

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if ! manifest_matches_target "${INSTALL_PATHS[$index]}" "${index}"; then
    echo "error: installed ${KINDS[$index]}/${EXTENSIONS[$index]} plugin failed its manifest check; rolling back" >&2
    rollback_install
    exit 70
  fi
done
for path in "${OBSOLETE_PATHS[@]}"; do
  if [[ -e "${path}" || -L "${path}" ]]; then
    echo "error: obsolete dump-reader target still shadows a promoted format handler: ${path}" >&2
    rollback_install
    exit 70
  fi
done

for ((index = 0; index < ${#MANAGED_PATHS[@]}; index++)); do
  cleanup_recovery_backup "$(target_label "${index}")" "${BACKUPS[$index]}"
done
for dir in "${OBSOLETE_DIRS[@]}"; do
  rmdir "${dir}" 2>/dev/null || true
done
trap - EXIT

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if [[ -n "${LINK_TARGETS[$index]}" ]]; then
    echo "installed: ${INSTALL_PATHS[$index]} -> ${LINK_TARGETS[$index]}"
  else
    echo "installed: ${INSTALL_PATHS[$index]}"
  fi
done
for path in "${OBSOLETE_PATHS[@]}"; do
  echo "retired: ${path}"
done
echo
echo "manifests reported by the installed plugins:"
"${INSTALL_PATHS[0]}" --info
"${INSTALL_PATHS[2]}" --info
echo
echo "verify discovery with:"
echo "  officecli plugins list"
