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
BIN_NAME="officecli-dump-reader-hwpx"
BUILT_BIN="${REPO_ROOT}/target/release/${BIN_NAME}"

KIND="dump-reader"
PLUGIN_ROOT="${HOME}/.officecli/plugins/${KIND}"
HWP_INSTALL_DIR="${PLUGIN_ROOT}/hwp"
HWPX_INSTALL_DIR="${PLUGIN_ROOT}/hwpx"
HWP_INSTALL_PATH="${HWP_INSTALL_DIR}/plugin"
HWPX_INSTALL_PATH="${HWPX_INSTALL_DIR}/plugin"

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

assert_install_directories_not_links() {
  for dir in "${HWPX_INSTALL_DIR}" "${HWP_INSTALL_DIR}"; do
    if [[ -L "${dir}" ]]; then
      echo "error: refusing reparseable install directory: ${dir}" >&2
      return 1
    fi
  done
}

case "${ACTION}" in
  uninstall)
    assert_install_directories_not_links || exit 73  # EX_CANTCREAT
    for entry in \
      "${HWP_INSTALL_PATH}|${HWP_INSTALL_DIR}" \
      "${HWPX_INSTALL_PATH}|${HWPX_INSTALL_DIR}"
    do
      path="${entry%%|*}"
      dir="${entry#*|}"
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
    echo "export OFFICECLI_PLUGIN_DUMP_READER_HWPX=\"${BUILT_BIN}\""
    echo "export OFFICECLI_PLUGIN_DUMP_READER_HWP=\"${BUILT_BIN}\""
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
mkdir -p "${HWPX_INSTALL_DIR}" "${HWP_INSTALL_DIR}"
chmod go-w "${HWPX_INSTALL_DIR}" "${HWP_INSTALL_DIR}"

STAGED_HWPX="$(mktemp "${HWPX_INSTALL_DIR}/.plugin.tmp.XXXXXX")"
STAGED_HWP=""
BACKUP_HWPX=""
BACKUP_HWP=""
HAD_HWPX=0
HAD_HWP=0
COMMITTED_HWPX=0
COMMITTED_HWP=0

cleanup_staging() {
  [[ -z "${STAGED_HWPX}" ]] || rm -f "${STAGED_HWPX}"
  [[ -z "${STAGED_HWP}" ]] || rm -f "${STAGED_HWP}"
}
trap cleanup_staging EXIT

install -m 0755 "${BUILT_BIN}" "${STAGED_HWPX}"
if ! "${STAGED_HWPX}" --info >/dev/null 2>&1; then
  echo "error: staged HWPX plugin failed its manifest check" >&2
  exit 70
fi

# The HWP resolver needs its own extension directory. Keep only one binary on
# Unix and use the protocol-supported relative symlink for the HWP path.
for _ in {1..32}; do
  candidate="${HWP_INSTALL_DIR}/.plugin-link.$$.$RANDOM"
  if ln -s "../hwpx/plugin" "${candidate}" 2>/dev/null; then
    STAGED_HWP="${candidate}"
    break
  fi
done
if [[ -z "${STAGED_HWP}" ]]; then
  echo "error: could not stage the HWP discovery link" >&2
  exit 73
fi

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
  if [[ "${COMMITTED_HWP}" -eq 1 ]]; then
    rm -f "${HWP_INSTALL_PATH}"
  fi
  if [[ "${COMMITTED_HWPX}" -eq 1 ]]; then
    rm -f "${HWPX_INSTALL_PATH}"
  fi
  if [[ "${HAD_HWPX}" -eq 1 && -n "${BACKUP_HWPX}" && ( -e "${BACKUP_HWPX}" || -L "${BACKUP_HWPX}" ) ]]; then
    mv "${BACKUP_HWPX}" "${HWPX_INSTALL_PATH}" || true
  fi
  if [[ "${HAD_HWP}" -eq 1 && -n "${BACKUP_HWP}" && ( -e "${BACKUP_HWP}" || -L "${BACKUP_HWP}" ) ]]; then
    mv "${BACKUP_HWP}" "${HWP_INSTALL_PATH}" || true
  fi
}

if [[ -e "${HWPX_INSTALL_PATH}" || -L "${HWPX_INSTALL_PATH}" ]]; then
  BACKUP_HWPX="$(unique_backup "${HWPX_INSTALL_DIR}" plugin)" || exit 73
  mv "${HWPX_INSTALL_PATH}" "${BACKUP_HWPX}"
  HAD_HWPX=1
fi
if [[ -e "${HWP_INSTALL_PATH}" || -L "${HWP_INSTALL_PATH}" ]]; then
  BACKUP_HWP="$(unique_backup "${HWP_INSTALL_DIR}" plugin)" || {
    rollback_install
    exit 73
  }
  if ! mv "${HWP_INSTALL_PATH}" "${BACKUP_HWP}"; then
    rollback_install
    exit 73
  fi
  HAD_HWP=1
fi

if ! mv "${STAGED_HWPX}" "${HWPX_INSTALL_PATH}"; then
  rollback_install
  exit 73
fi
STAGED_HWPX=""
COMMITTED_HWPX=1
if ! mv "${STAGED_HWP}" "${HWP_INSTALL_PATH}"; then
  rollback_install
  exit 73
fi
STAGED_HWP=""
COMMITTED_HWP=1

if ! "${HWPX_INSTALL_PATH}" --info >/dev/null 2>&1 || \
   ! "${HWP_INSTALL_PATH}" --info >/dev/null 2>&1; then
  echo "error: installed plugin failed its manifest check; rolling back" >&2
  rollback_install
  exit 70
fi

[[ -z "${BACKUP_HWPX}" ]] || rm -f "${BACKUP_HWPX}"
[[ -z "${BACKUP_HWP}" ]] || rm -f "${BACKUP_HWP}"
trap - EXIT

echo "installed: ${HWPX_INSTALL_PATH}"
echo "installed: ${HWP_INSTALL_PATH} -> ../hwpx/plugin"
echo
echo "manifest reported by the installed plugin:"
"${HWPX_INSTALL_PATH}" --info
echo
echo "verify discovery with:"
echo "  officecli plugins list"
