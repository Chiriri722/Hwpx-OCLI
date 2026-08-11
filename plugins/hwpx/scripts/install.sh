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
EXT="hwpx"
INSTALL_DIR="${HOME}/.officecli/plugins/${KIND}/${EXT}"
INSTALL_PATH="${INSTALL_DIR}/plugin"

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

case "${ACTION}" in
  uninstall)
    if [[ -e "${INSTALL_PATH}" ]]; then
      rm -f "${INSTALL_PATH}"
      echo "removed ${INSTALL_PATH}"
      # 빈 디렉터리만 정리한다. 다른 플러그인을 건드리지 않는다.
      rmdir "${INSTALL_DIR}" 2>/dev/null || true
    else
      echo "not installed: ${INSTALL_PATH}"
    fi
    exit 0
    ;;

  print-env)
    # §3 1순위: 환경변수. <KIND>와 <EXT>는 대문자, 하이픈은 밑줄.
    echo "export OFFICECLI_PLUGIN_DUMP_READER_HWPX=\"${BUILT_BIN}\""
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
  ( cd "${REPO_ROOT}" && cargo build --release )
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

mkdir -p "${INSTALL_DIR}"
install -m 0755 "${BUILT_BIN}" "${INSTALL_PATH}"

echo "installed: ${INSTALL_PATH}"
echo
echo "manifest reported by the installed plugin:"
"${INSTALL_PATH}" --info
echo
echo "verify discovery with:"
echo "  officecli plugins list"
