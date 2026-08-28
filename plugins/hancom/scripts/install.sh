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
BIN_NAME="officecli-hancom-hwp"
BUILT_BIN="${REPO_ROOT}/target/release/${BIN_NAME}"

KIND="dump-reader"
EXTENSIONS=("hwpx" "hwp" "owpml" "hml")
ENV_SUFFIXES=("HWPX" "HWP" "OWPML" "HML")
CANONICAL_INDEX=0
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
PLUGIN_ROOT="${PLUGINS_DIR}/${KIND}"
INSTALL_DIRS=()
INSTALL_PATHS=()
LINK_TARGETS=()
for extension in "${EXTENSIONS[@]}"; do
  INSTALL_DIRS+=("${PLUGIN_ROOT}/${extension}")
  INSTALL_PATHS+=("${PLUGIN_ROOT}/${extension}/plugin")
  if [[ "${extension}" == "hwpx" ]]; then
    LINK_TARGETS+=("")
  else
    LINK_TARGETS+=("../hwpx/plugin")
  fi
done

assert_install_directories_not_links() {
  for dir in \
    "${OFFICECLI_DIR}" \
    "${PLUGINS_DIR}" \
    "${PLUGIN_ROOT}" \
    "${INSTALL_DIRS[@]}"
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

assert_install_targets_safe() {
  local index path expected_link
  for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
    path="${INSTALL_PATHS[$index]}"
    expected_link="${LINK_TARGETS[$index]}"
    if [[ -L "${path}" ]]; then
      if [[ -z "${expected_link}" ]]; then
        echo "error: refusing reparseable install target: ${path}" >&2
        return 1
      fi
      if [[ "$(readlink "${path}")" != "${expected_link}" || \
            ! -f "${INSTALL_PATHS[$CANONICAL_INDEX]}" ]]; then
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
    # Remove links before the canonical binary so every preflighted link remains
    # resolvable until its own removal.
    for ((index = ${#EXTENSIONS[@]} - 1; index >= 0; index--)); do
      path="${INSTALL_PATHS[$index]}"
      dir="${INSTALL_DIRS[$index]}"
      if [[ -e "${path}" || -L "${path}" ]]; then
        rm -f "${path}"
        echo "removed ${path}"
      else
        echo "not installed: ${path}"
      fi
      # 빈 확장자 디렉터리만 정리한다. 다른 플러그인을 건드리지 않는다.
      rmdir "${dir}" 2>/dev/null || true
    done
    exit 0
    ;;

  print-env)
    # §3 1순위: 환경변수. <KIND>와 <EXT>는 대문자, 하이픈은 밑줄.
    for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
      echo "export OFFICECLI_PLUGIN_DUMP_READER_${ENV_SUFFIXES[$index]}=\"${BUILT_BIN}\""
    done
    exit 0
    ;;
esac

if [[ "${DO_BUILD}" -eq 1 ]]; then
  echo "building release binary..."
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

if [[ ! -x "${BUILT_BIN}" ]]; then
  echo "error: binary not found at ${BUILT_BIN}" >&2
  echo "run without --no-build, or run: cargo build --release" >&2
  exit 69
fi

# 설치 전에 매니페스트가 실제로 유효한지 확인한다.
# 매니페스트가 깨진 플러그인은 메인이 조용히 무시하거나 exit 5로 거부한다.
if ! "${BUILT_BIN}" --info >/dev/null 2>&1; then
  echo "error: '${BIN_NAME} --info' failed. refusing to install." >&2
  exit 70  # EX_SOFTWARE
fi

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
  BACKUPS+=("")
  HAD_EXISTING+=(0)
  COMMITTED+=(0)
done

cleanup_staging() {
  local index staged_path
  for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
    staged_path="${STAGED[$index]}"
    if [[ -n "${staged_path}" ]] && ! rm -f "${staged_path}"; then
      echo "warning: could not remove staged ${ENV_SUFFIXES[$index]} target: ${staged_path}" >&2
    fi
  done
  return 0
}
trap cleanup_staging EXIT

STAGED[$CANONICAL_INDEX]="$(mktemp "${INSTALL_DIRS[$CANONICAL_INDEX]}/.plugin.tmp.XXXXXX")"
install -m 0755 "${BUILT_BIN}" "${STAGED[$CANONICAL_INDEX]}"
if ! "${STAGED[$CANONICAL_INDEX]}" --info >/dev/null 2>&1; then
  echo "error: staged HWPX plugin failed its manifest check" >&2
  exit 70
fi

# Each resolver needs its own extension directory. Keep one physical binary on
# Unix and use protocol-supported relative links for every other extension.
for ((index = 1; index < ${#EXTENSIONS[@]}; index++)); do
  for _ in {1..32}; do
    candidate="${INSTALL_DIRS[$index]}/.plugin-link.$$.$RANDOM"
    if ln -s "${LINK_TARGETS[$index]}" "${candidate}" 2>/dev/null; then
      STAGED[$index]="${candidate}"
      break
    fi
  done
  if [[ -z "${STAGED[$index]}" ]]; then
    echo "error: could not stage the ${ENV_SUFFIXES[$index]} discovery link" >&2
    exit 73
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

rollback_install() {
  local index path backup
  local -a unrecovered=()
  local rollback_incomplete=0
  for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
    unrecovered+=(0)
  done

  for ((index = ${#EXTENSIONS[@]} - 1; index >= 0; index--)); do
    path="${INSTALL_PATHS[$index]}"
    if [[ "${COMMITTED[$index]}" -eq 1 ]] && ! rm -f "${path}"; then
      echo "warning: could not remove committed ${ENV_SUFFIXES[$index]} target during rollback: ${path}" >&2
      unrecovered[$index]=1
    fi
  done

  for ((index = ${#EXTENSIONS[@]} - 1; index >= 0; index--)); do
    if [[ "${HAD_EXISTING[$index]}" -ne 1 ]]; then
      continue
    fi
    path="${INSTALL_PATHS[$index]}"
    backup="${BACKUPS[$index]}"
    if [[ -n "${backup}" && ( -e "${backup}" || -L "${backup}" ) ]]; then
      if mv "${backup}" "${path}"; then
        BACKUPS[$index]=""
        unrecovered[$index]=0
      else
        echo "error: could not restore ${ENV_SUFFIXES[$index]} recovery backup: ${backup}" >&2
        unrecovered[$index]=1
      fi
    else
      echo "error: ${ENV_SUFFIXES[$index]} recovery backup is missing during rollback: ${backup}" >&2
      unrecovered[$index]=1
    fi
  done

  for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
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

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  path="${INSTALL_PATHS[$index]}"
  if [[ -e "${path}" || -L "${path}" ]]; then
    BACKUPS[$index]="$(unique_backup "${INSTALL_DIRS[$index]}" plugin)" || {
      rollback_install
      exit 73
    }
    if ! mv "${path}" "${BACKUPS[$index]}"; then
      if [[ -e "${BACKUPS[$index]}" || -L "${BACKUPS[$index]}" ]]; then
        HAD_EXISTING[$index]=1
      fi
      rollback_install
      exit 73
    fi
    HAD_EXISTING[$index]=1
  fi
done

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if ! mv "${STAGED[$index]}" "${INSTALL_PATHS[$index]}"; then
    if [[ ! -e "${STAGED[$index]}" && ! -L "${STAGED[$index]}" && \
          ( -e "${INSTALL_PATHS[$index]}" || -L "${INSTALL_PATHS[$index]}" ) ]]; then
      STAGED[$index]=""
      COMMITTED[$index]=1
    fi
    rollback_install
    exit 73
  fi
  STAGED[$index]=""
  COMMITTED[$index]=1
done

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if ! "${INSTALL_PATHS[$index]}" --info >/dev/null 2>&1; then
    echo "error: installed ${ENV_SUFFIXES[$index]} plugin failed its manifest check; rolling back" >&2
    rollback_install
    exit 70
  fi
done

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  cleanup_recovery_backup "${ENV_SUFFIXES[$index]}" "${BACKUPS[$index]}"
done
trap - EXIT

for ((index = 0; index < ${#EXTENSIONS[@]}; index++)); do
  if [[ -n "${LINK_TARGETS[$index]}" ]]; then
    echo "installed: ${INSTALL_PATHS[$index]} -> ${LINK_TARGETS[$index]}"
  else
    echo "installed: ${INSTALL_PATHS[$index]}"
  fi
done
echo
echo "manifest reported by the installed plugin:"
"${INSTALL_PATHS[$CANONICAL_INDEX]}" --info
echo
echo "verify discovery with:"
echo "  officecli plugins list"
