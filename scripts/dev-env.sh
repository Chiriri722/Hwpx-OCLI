#!/usr/bin/env bash

# Source this file from the repository root:
#   source scripts/dev-env.sh

if [[ -n "${ZSH_VERSION:-}" ]]; then
  dev_env_script_path="${(%):-%N}"
else
  dev_env_script_path="${BASH_SOURCE[0]}"
fi

dev_env_script_dir="$(cd "$(dirname "${dev_env_script_path}")" && pwd -P)"
dev_env_repo_root="$(cd "${dev_env_script_dir}/.." && pwd -P)"

export DOTNET_ROOT="${dev_env_repo_root}/.dotnet"
export DOTNET_CLI_HOME="${dev_env_repo_root}/.dotnet-cli-home"
export NUGET_PACKAGES="${dev_env_repo_root}/.nuget/packages"
export DOTNET_CLI_TELEMETRY_OPTOUT=1
export DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1
export DOTNET_NOLOGO=1
case ":${PATH}:" in
  *":${DOTNET_ROOT}:"*) ;;
  *) export PATH="${DOTNET_ROOT}:${PATH}" ;;
esac

dev_env_cargo_bin="${CARGO_HOME:-${HOME}/.cargo}/bin"
if [[ -d "${dev_env_cargo_bin}" ]]; then
  case ":${PATH}:" in
    *":${dev_env_cargo_bin}:"*) ;;
    *) export PATH="${dev_env_cargo_bin}:${PATH}" ;;
  esac
fi

unset dev_env_script_path dev_env_script_dir dev_env_repo_root dev_env_cargo_bin
