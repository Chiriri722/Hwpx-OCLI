#!/usr/bin/env bash
set -euo pipefail

bootstrap_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bootstrap_repo_root="$(cd "${bootstrap_script_dir}/.." && pwd -P)"
bootstrap_dotnet_version="10.0.302"
bootstrap_dotnet_dir="${bootstrap_repo_root}/.dotnet"
bootstrap_dotnet_cli_home="${bootstrap_repo_root}/.dotnet-cli-home"
bootstrap_current_dotnet_version=""

if [[ -x "${bootstrap_dotnet_dir}/dotnet" ]]; then
  bootstrap_current_dotnet_version="$(
    DOTNET_CLI_HOME="${bootstrap_dotnet_cli_home}" \
      "${bootstrap_dotnet_dir}/dotnet" --version 2>/dev/null || true
  )"
fi

if [[ "${bootstrap_current_dotnet_version}" != "${bootstrap_dotnet_version}" ]]; then
  bootstrap_installer="$(mktemp -t hwpx-ocli-dotnet-install.XXXXXX)"
  trap 'rm -f "${bootstrap_installer}"' EXIT
  curl -fsSL https://dot.net/v1/dotnet-install.sh -o "${bootstrap_installer}"
  bash "${bootstrap_installer}" \
    --version "${bootstrap_dotnet_version}" \
    --install-dir "${bootstrap_dotnet_dir}" \
    --no-path
fi

# shellcheck source=dev-env.sh
source "${bootstrap_script_dir}/dev-env.sh"

if ! command -v cargo >/dev/null 2>&1; then
  printf '%s\n' \
    'Rust is required for plugins/hancom.' \
    'Install rustup from https://rustup.rs, then rerun this script.' >&2
  exit 1
fi

if command -v rustup >/dev/null 2>&1; then
  rustup component add clippy rustfmt
fi

printf 'dotnet: %s\n' "$(dotnet --version)"
printf 'cargo:  %s\n' "$(cargo --version)"
printf 'rustc:  %s\n' "$(rustc --version)"
