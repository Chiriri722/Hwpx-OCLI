#!/usr/bin/env python3
"""Enforce immutable external refs in workflows and composite actions."""

from __future__ import annotations

import os
import re
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import yaml
from yaml.nodes import MappingNode, Node, ScalarNode, SequenceNode


ROOT = Path(__file__).resolve().parents[1]
FULL_COMMIT_SHA = re.compile(r"^[0-9a-fA-F]{40}$")
IMMUTABLE_DOCKER_REF = re.compile(
    r"^docker://[^\s@]+@sha256:[0-9a-fA-F]{64}$"
)
MAX_WORKFLOW_BYTES = 1024 * 1024
MAX_YAML_EVENTS = 20_000
MAX_YAML_DEPTH = 64
MAX_YAML_ALIASES = 256
MAX_YAML_NODES = 10_000
MAX_MERGE_EXPANSION = 4_096
MAX_TOTAL_MERGE_EXPANSION = 20_000
YAML_MERGE_TAG = "tag:yaml.org,2002:merge"


class YamlComplexityError(ValueError):
    """Raised when a YAML document exceeds deterministic parser budgets."""


class YamlStructureError(ValueError):
    """Raised when ambiguous YAML cannot be checked safely."""


def _one_line(value: object, limit: int = 256) -> str:
    """Render untrusted YAML values without multiline or unbounded diagnostics."""
    text = str(value).replace("\\", "\\\\").replace("\r", "\\r").replace("\n", "\\n")
    if len(text) > limit:
        return text[:limit] + "..."
    return text


def _merge_targets(node: Node) -> list[MappingNode]:
    """Return mapping nodes referenced by one YAML merge value."""
    if isinstance(node, MappingNode):
        return [node]
    if isinstance(node, SequenceNode) and all(
        isinstance(item, MappingNode) for item in node.value
    ):
        return list(node.value)
    raise YamlStructureError("merge values must be a mapping or sequence of mappings")


def _validate_yaml_complexity(text: str) -> None:
    """Bound parse depth, aliases, nodes, and effective merge expansion."""
    depth = 0
    aliases = 0
    for event_count, event in enumerate(
        yaml.parse(text, Loader=yaml.SafeLoader), start=1
    ):
        if event_count > MAX_YAML_EVENTS:
            raise YamlComplexityError(
                f"event count exceeds {MAX_YAML_EVENTS}"
            )
        if isinstance(event, (yaml.MappingStartEvent, yaml.SequenceStartEvent)):
            depth += 1
            if depth > MAX_YAML_DEPTH:
                raise YamlComplexityError(f"nesting exceeds {MAX_YAML_DEPTH}")
        elif isinstance(event, (yaml.MappingEndEvent, yaml.SequenceEndEvent)):
            depth -= 1
        elif isinstance(event, yaml.AliasEvent):
            aliases += 1
            if aliases > MAX_YAML_ALIASES:
                raise YamlComplexityError(
                    f"alias count exceeds {MAX_YAML_ALIASES}"
                )

    document = yaml.compose(text, Loader=yaml.SafeLoader)
    if document is None:
        return

    seen: set[int] = set()
    active: set[int] = set()
    mapping_nodes: list[MappingNode] = []

    def visit(node: Node, node_depth: int) -> None:
        if node_depth > MAX_YAML_DEPTH:
            raise YamlComplexityError(f"node depth exceeds {MAX_YAML_DEPTH}")
        identity = id(node)
        if identity in active:
            raise YamlComplexityError("cyclic aliases are not supported")
        if identity in seen:
            return
        if len(seen) >= MAX_YAML_NODES:
            raise YamlComplexityError(f"node count exceeds {MAX_YAML_NODES}")

        seen.add(identity)
        active.add(identity)
        try:
            if isinstance(node, MappingNode):
                mapping_nodes.append(node)
                keys: set[tuple[str, str]] = set()
                for key_node, value_node in node.value:
                    if not isinstance(key_node, ScalarNode):
                        raise YamlStructureError("mapping keys must be scalars")
                    key = (key_node.tag, key_node.value)
                    if key in keys:
                        raise YamlStructureError(
                            f"duplicate mapping key: {_one_line(key_node.value)}"
                        )
                    keys.add(key)
                    visit(key_node, node_depth + 1)
                    visit(value_node, node_depth + 1)
            elif isinstance(node, SequenceNode):
                for item in node.value:
                    visit(item, node_depth + 1)
        finally:
            active.remove(identity)

    visit(document, 1)

    merge_costs: dict[int, int] = {}
    merge_active: set[int] = set()

    def merge_expansion(node: MappingNode) -> int:
        identity = id(node)
        if identity in merge_costs:
            return merge_costs[identity]
        if identity in merge_active:
            raise YamlComplexityError("cyclic YAML merges are not supported")

        merge_active.add(identity)
        try:
            explicit_pairs = sum(
                1 for key_node, _value_node in node.value
                if key_node.tag != YAML_MERGE_TAG
            )
            cost = max(1, explicit_pairs)
            for key_node, value_node in node.value:
                if key_node.tag != YAML_MERGE_TAG:
                    continue
                for target in _merge_targets(value_node):
                    cost += merge_expansion(target)
                    if cost > MAX_MERGE_EXPANSION:
                        raise YamlComplexityError(
                            f"merge expansion exceeds {MAX_MERGE_EXPANSION}"
                        )
            merge_costs[identity] = cost
            return cost
        finally:
            merge_active.remove(identity)

    total_merge_expansion = 0
    for mapping_node in mapping_nodes:
        total_merge_expansion += merge_expansion(mapping_node)
        if total_merge_expansion > MAX_TOTAL_MERGE_EXPANSION:
            raise YamlComplexityError(
                "total merge expansion exceeds "
                f"{MAX_TOTAL_MERGE_EXPANSION}"
            )


def _read_source_text(
    source_path: Path, relative_path: str
) -> tuple[str | None, list[str]]:
    """Read one regular, non-symlink source with a strict byte ceiling."""
    try:
        if source_path.is_symlink():
            return None, [f"{relative_path}: symbolic links are not allowed"]
        metadata = source_path.lstat()
        if not stat.S_ISREG(metadata.st_mode):
            return None, [f"{relative_path}: source is not a regular file"]

        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
        flags |= getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(source_path, flags)
        try:
            if not stat.S_ISREG(os.fstat(descriptor).st_mode):
                return None, [f"{relative_path}: source is not a regular file"]
            with os.fdopen(descriptor, "rb") as stream:
                descriptor = -1
                content = stream.read(MAX_WORKFLOW_BYTES + 1)
        finally:
            if descriptor >= 0:
                os.close(descriptor)
    except OSError as error:
        return None, [f"{relative_path}: cannot read source: {_one_line(error)}"]

    if len(content) > MAX_WORKFLOW_BYTES:
        return None, [f"{relative_path}: source exceeds {MAX_WORKFLOW_BYTES} bytes"]
    try:
        return content.decode("utf-8"), []
    except UnicodeDecodeError as error:
        return None, [f"{relative_path}: source is not UTF-8: {_one_line(error)}"]


def _semantic_uses_values(workflow: object) -> list[tuple[str, object]]:
    """Return action references from executable workflow positions only."""
    if not isinstance(workflow, dict):
        return []

    values: list[tuple[str, object]] = []
    jobs = workflow.get("jobs")
    if isinstance(jobs, dict):
        for job_name, raw_job in jobs.items():
            if not isinstance(raw_job, dict):
                continue

            job_location = f"jobs.{_one_line(job_name)}"
            if "uses" in raw_job:
                values.append((f"{job_location}.uses", raw_job["uses"]))

            steps = raw_job.get("steps")
            if not isinstance(steps, list):
                continue
            for index, raw_step in enumerate(steps):
                if isinstance(raw_step, dict) and "uses" in raw_step:
                    values.append(
                        (f"{job_location}.steps[{index}].uses", raw_step["uses"])
                    )

    runs = workflow.get("runs")
    if isinstance(runs, dict) and isinstance(runs.get("steps"), list):
        steps = runs["steps"]
        for index, raw_step in enumerate(steps):
            if isinstance(raw_step, dict) and "uses" in raw_step:
                values.append((f"runs.steps[{index}].uses", raw_step["uses"]))

    return values


def workflow_action_pin_violations(text: str, relative_path: str) -> list[str]:
    """Return mutable external action refs from one parsed action document."""
    if len(text.encode("utf-8")) > MAX_WORKFLOW_BYTES:
        return [f"{relative_path}: workflow exceeds {MAX_WORKFLOW_BYTES} bytes"]

    try:
        _validate_yaml_complexity(text)
        workflow = yaml.safe_load(text)
    except YamlComplexityError as error:
        return [f"{relative_path}: YAML complexity limit: {_one_line(error)}"]
    except YamlStructureError as error:
        return [f"{relative_path}: invalid YAML structure: {_one_line(error)}"]
    except (yaml.YAMLError, RecursionError) as error:
        return [f"{relative_path}: invalid YAML: {_one_line(error)}"]

    violations: list[str] = []
    for location, raw_value in _semantic_uses_values(workflow):
        if not isinstance(raw_value, str):
            violations.append(
                f"{relative_path}:{location}: <non-string uses value>"
            )
            continue

        value = raw_value.strip()
        if value.startswith(("./", "$/")):
            continue
        if value.startswith("docker://"):
            if IMMUTABLE_DOCKER_REF.fullmatch(value) is not None:
                continue
            rendered = _one_line(value) if value else "<missing uses value>"
            violations.append(f"{relative_path}:{location}: {rendered}")
            continue

        action, separator, ref = value.rpartition("@")
        if (
            not separator
            or "/" not in action
            or FULL_COMMIT_SHA.fullmatch(ref) is None
        ):
            rendered = _one_line(value) if value else "<missing uses value>"
            violations.append(f"{relative_path}:{location}: {rendered}")
    return violations


def external_action_pin_violations(root: Path | None = None) -> list[str]:
    """Return workflow/action locations whose external ``uses`` ref is mutable."""
    if root is None:
        configured_root = os.environ.get("ACTION_PIN_AUDIT_ROOT")
        root = Path(configured_root) if configured_root else ROOT
    try:
        root = root.resolve(strict=True)
    except OSError as error:
        return [f"audit root cannot be resolved: {_one_line(error)}"]
    if not root.is_dir():
        return [f"audit root is not a directory: {_one_line(root)}"]
    violations: list[str] = []
    walk_errors: list[OSError] = []
    source_paths: list[Path] = []

    github_directory = root / ".github"
    workflows = github_directory / "workflows"
    if github_directory.is_symlink():
        violations.append(".github: symbolic links are not allowed")
    elif github_directory.exists() and not github_directory.is_dir():
        violations.append(".github: source is not a directory")
    elif workflows.is_symlink():
        violations.append(".github/workflows: symbolic links are not allowed")
    elif workflows.exists() and not workflows.is_dir():
        violations.append(".github/workflows: source is not a directory")

    for directory, directory_names, file_names in os.walk(
        root, topdown=True, onerror=walk_errors.append, followlinks=False
    ):
        directory_path = Path(directory)
        safe_directories: list[str] = []
        for name in sorted(directory_names):
            candidate = directory_path / name
            if name == ".git" or candidate.is_symlink():
                continue
            is_junction = getattr(os.path, "isjunction", lambda _path: False)
            if is_junction(candidate):
                continue
            safe_directories.append(name)
        directory_names[:] = safe_directories
        for name in file_names:
            relative_directory = directory_path.relative_to(root).as_posix()
            is_workflow = (
                relative_directory == ".github/workflows"
                and Path(name).suffix in {".yml", ".yaml"}
            )
            if is_workflow or name.casefold() in {"action.yml", "action.yaml"}:
                source_paths.append(directory_path / name)

    source_paths = sorted(set(source_paths))
    for error in walk_errors:
        violations.append(f"repository scan failed: {_one_line(error)}")

    for source_path in source_paths:
        relative_path = source_path.relative_to(root).as_posix()
        text, read_violations = _read_source_text(source_path, relative_path)
        violations.extend(read_violations)
        if text is not None:
            violations.extend(workflow_action_pin_violations(text, relative_path))

    return violations


class WorkflowActionPinsTest(unittest.TestCase):
    def test_external_actions_use_full_commit_shas(self) -> None:
        violations = external_action_pin_violations()
        self.assertEqual(
            [],
            violations,
            "External workflow actions must use full 40-hex commit SHAs:\n"
            + "\n".join(violations),
        )

    def test_pull_requests_execute_the_base_trusted_checker(self) -> None:
        workflow_path = ROOT / ".github" / "workflows" / "action-pins.yml"
        workflow, read_violations = _read_source_text(
            workflow_path, ".github/workflows/action-pins.yml"
        )

        self.assertEqual([], read_violations)
        self.assertIsNotNone(workflow)
        assert workflow is not None
        self.assertIn("  pull_request_target:\n", workflow)
        self.assertNotIn("  pull_request:\n", workflow)
        self.assertNotIn("    paths:\n", workflow)
        self.assertIn("trusted/scripts/test-workflow-action-pins.py", workflow)
        self.assertIn("ACTION_PIN_AUDIT_ROOT", workflow)
        self.assertIn("allow-unsafe-pr-checkout: true", workflow)

    def test_flow_style_and_quoted_uses_keys_cannot_bypass_the_check(self) -> None:
        pinned_sha = "0123456789abcdef0123456789abcdef01234567"
        workflow = (
            "name: flow syntax\n"
            "jobs:\n"
            "  verify:\n"
            "    steps: [{ uses: owner/compact@latest }]\n"
            "  variants:\n"
            "    steps:\n"
            "      - { uses: actions/checkout@v5 }\n"
            "      - \"uses\": 'owner/tool@main'\n"
            "      - uses: './local-action'\n"
            f"      - {{ name: pinned, uses: owner/tool@{pinned_sha} }}\n"
            "      - run: \"echo '{ uses: owner/tool@mutable }'\"\n"
            "      # uses: owner/commented@mutable\n"
        )
        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/flow.yml"
        )

        self.assertEqual(3, len(violations), violations)
        self.assertTrue(any("actions/checkout@v5" in item for item in violations))
        self.assertTrue(any("owner/compact@latest" in item for item in violations))
        self.assertTrue(any("owner/tool@main" in item for item in violations))

    def test_yaml_decoded_keys_and_reusable_workflows_are_checked(self) -> None:
        workflow = (
            "name: decoded keys\n"
            "on: workflow_dispatch\n"
            "x-step: &mutable-step\n"
            "  uses: owner/anchored@dev\n"
            "jobs:\n"
            "  escaped:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - <<: *mutable-step\n"
            "      - \"\\u0075ses\": owner/escaped@main\n"
            "  reusable:\n"
            "    uses: owner/repository/.github/workflows/build.yml@main\n"
        )

        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/decoded.yml"
        )

        self.assertEqual(3, len(violations), violations)
        self.assertTrue(any("owner/anchored@dev" in item for item in violations))
        self.assertTrue(any("owner/escaped@main" in item for item in violations))
        self.assertTrue(any("workflows/build.yml@main" in item for item in violations))

    def test_scalar_text_is_ignored_and_folded_pinned_refs_are_resolved(self) -> None:
        pinned_sha = "0" * 40
        workflow = (
            "name: semantic values\n"
            "on: workflow_dispatch\n"
            "env:\n"
            "  uses: owner/not-an-action@main\n"
            "jobs:\n"
            "  verify:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps: [{ run: \"echo { uses: owner/tool@main }\" }]\n"
            "  folded:\n"
            "    runs-on: ubuntu-latest\n"
            "    steps:\n"
            "      - uses: >-\n"
            f"          owner/action@{pinned_sha}\n"
        )

        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/semantic.yml"
        )

        self.assertEqual([], violations)

    def test_composite_action_steps_are_checked(self) -> None:
        action = (
            "name: composite\n"
            "description: test action\n"
            "runs:\n"
            "  using: composite\n"
            "  steps:\n"
            "    - uses: owner/composite-dependency@main\n"
        )

        violations = workflow_action_pin_violations(
            action, ".github/actions/example/action.yml"
        )

        self.assertEqual(1, len(violations), violations)
        self.assertIn("owner/composite-dependency@main", violations[0])

    def test_local_self_action_references_are_exempt(self) -> None:
        workflow = (
            "name: local actions\n"
            "jobs:\n"
            "  verify:\n"
            "    steps:\n"
            "      - uses: ./ci/checkout-action\n"
            "      - uses: $/.github/actions/no-checkout-action\n"
        )

        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/local.yml"
        )

        self.assertEqual([], violations)

    def test_docker_actions_require_an_immutable_sha256_digest(self) -> None:
        digest = "a" * 64
        workflow = (
            "name: docker actions\n"
            "jobs:\n"
            "  verify:\n"
            "    steps:\n"
            f"      - uses: docker://ghcr.io/example/tool@sha256:{digest}\n"
            "      - uses: docker://alpine:latest\n"
        )

        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/docker.yml"
        )

        self.assertEqual(1, len(violations), violations)
        self.assertIn("docker://alpine:latest", violations[0])

    def test_action_metadata_is_found_outside_github_actions(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            workflow_directory = root / ".github" / "workflows"
            action_directory = root / "ci" / "my-action"
            workflow_directory.mkdir(parents=True)
            action_directory.mkdir(parents=True)
            (action_directory / "action.yml").write_text(
                "name: nested action\n"
                "runs:\n"
                "  using: composite\n"
                "  steps:\n"
                "    - uses: owner/dependency@main\n",
                encoding="utf-8",
            )

            violations = external_action_pin_violations(root)

        self.assertEqual(1, len(violations), violations)
        self.assertIn("ci/my-action/action.yml", violations[0])
        self.assertIn("owner/dependency@main", violations[0])

    def test_windows_case_insensitive_action_metadata_is_always_checked(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            action_directory = root / "ci" / "windows-action"
            action_directory.mkdir(parents=True)
            (action_directory / "Action.yml").write_text(
                "name: Windows action\n"
                "runs:\n"
                "  using: composite\n"
                "  steps:\n"
                "    - uses: owner/windows-dependency@main\n",
                encoding="utf-8",
            )

            violations = external_action_pin_violations(root)

        self.assertEqual(1, len(violations), violations)
        self.assertIn("ci/windows-action/Action.yml", violations[0])

    def test_trusted_checker_can_audit_an_explicit_candidate_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            action_directory = root / "custom-action"
            action_directory.mkdir()
            (action_directory / "action.yml").write_text(
                "name: candidate action\n"
                "runs:\n"
                "  using: composite\n"
                "  steps:\n"
                "    - uses: owner/candidate@main\n",
                encoding="utf-8",
            )
            with mock.patch.dict(
                os.environ, {"ACTION_PIN_AUDIT_ROOT": str(root)}
            ):
                violations = external_action_pin_violations()

        self.assertEqual(1, len(violations), violations)
        self.assertIn("custom-action/action.yml", violations[0])

    def test_merge_alias_bomb_is_rejected_before_construction(self) -> None:
        levels = ["seed: &seed {uses: owner/dependency@main}"]
        previous = "seed"
        for index in range(10):
            current = f"level{index}"
            levels.append(
                f"{current}: &{current} {{<<: [*{previous}, *{previous}, "
                f"*{previous}, *{previous}, *{previous}]}}"
            )
            previous = current
        levels.extend(
            [
                "jobs:",
                "  verify:",
                "    steps:",
                f"      - <<: *{previous}",
            ]
        )

        violations = workflow_action_pin_violations(
            "\n".join(levels), ".github/workflows/merge-bomb.yml"
        )

        self.assertEqual(1, len(violations), violations)
        self.assertIn("complexity", violations[0].lower())

    def test_total_merge_expansion_is_bounded_across_the_document(self) -> None:
        lines: list[str] = []
        for chain in range(5):
            previous = f"seed{chain}"
            lines.append(f"{previous}: &{previous} {{value: {chain}}}")
            for level in range(5):
                current = f"chain{chain}level{level}"
                lines.append(
                    f"{current}: &{current} {{<<: [*{previous}, *{previous}, "
                    f"*{previous}, *{previous}, *{previous}]}}"
                )
                previous = current
        lines.append("jobs: {}")

        violations = workflow_action_pin_violations(
            "\n".join(lines), ".github/workflows/aggregate-merge-bomb.yml"
        )

        self.assertEqual(1, len(violations), violations)
        self.assertIn("complexity", violations[0].lower())

    def test_merge_cost_includes_explicit_mapping_width(self) -> None:
        explicit_pairs = ", ".join(f"key{index}: {index}" for index in range(300))
        aliases = ", ".join("*wide" for _ in range(16))
        workflow = (
            f"wide: &wide {{{explicit_pairs}}}\n"
            f"expanded: &expanded {{<<: [{aliases}]}}\n"
            "jobs: {}\n"
        )

        violations = workflow_action_pin_violations(
            workflow, ".github/workflows/wide-merge.yml"
        )

        self.assertEqual(1, len(violations), violations)
        self.assertIn("complexity", violations[0].lower())

    def test_bounded_reader_rejects_oversized_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            source_path = Path(temporary_directory) / "action.yml"
            source_path.write_bytes(b"a" * (MAX_WORKFLOW_BYTES + 1))

            text, violations = _read_source_text(source_path, "action.yml")

        self.assertIsNone(text)
        self.assertEqual(1, len(violations), violations)
        self.assertIn("exceeds", violations[0])

    def test_bounded_reader_rejects_symlinks_before_opening(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            source_path = Path(temporary_directory) / "action.yml"
            source_path.write_text("name: action\n", encoding="utf-8")
            with mock.patch.object(Path, "is_symlink", return_value=True):
                text, violations = _read_source_text(source_path, "action.yml")

        self.assertIsNone(text)
        self.assertEqual(1, len(violations), violations)
        self.assertIn("symbolic link", violations[0])

    def test_workflow_directory_symlink_is_rejected_without_globbing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory) / "candidate"
            (root / ".github").mkdir(parents=True)
            workflow_link = root / ".github" / "workflows"
            original_is_symlink = Path.is_symlink

            def simulated_is_symlink(path: Path) -> bool:
                return path == workflow_link or original_is_symlink(path)

            with mock.patch.object(Path, "is_symlink", simulated_is_symlink):
                violations = external_action_pin_violations(root)

        self.assertEqual(1, len(violations), violations)
        self.assertIn(".github/workflows", violations[0])
        self.assertIn("symbolic link", violations[0])


if __name__ == "__main__":
    unittest.main()
