#!/usr/bin/env python3
"""Stamp an otherwise profile-compatible OOXML fixture with exact Hancom v12 markers."""

from __future__ import annotations

import argparse
import posixpath
import shutil
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path, PurePosixPath


APP_NS = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"
SPREADSHEET_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
HANCOM_SHEET_NS = "http://schemas.haansoft.com/office/spreadsheet/8.0"
PACKAGE_RELS_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
OFFICE_REL = (
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
)
APP_REL = (
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties"
)
CUSTOM_REL = (
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties"
)
CUSTOM_PROPERTIES_PART = "docProps/custom.xml"
CONTENT_TYPES_NS = "http://schemas.openxmlformats.org/package/2006/content-types"
CELL_MAIN_TYPE = (
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
)
SHOW_MAIN_TYPE = (
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"
)


def stamp_app_properties(payload: bytes, family: str) -> bytes:
    root = ET.fromstring(payload)
    expected_root = f"{{{APP_NS}}}Properties"
    if root.tag != expected_root:
        raise ValueError("docProps/app.xml has an unexpected root")

    applications = root.findall(f"{{{APP_NS}}}Application")
    versions = root.findall(f"{{{APP_NS}}}AppVersion")
    if len(applications) > 1 or len(versions) > 1:
        raise ValueError("docProps/app.xml contains duplicate producer properties")
    if applications:
        application = applications[0]
    else:
        application = ET.SubElement(root, f"{{{APP_NS}}}Application")
    if versions:
        version = versions[0]
    else:
        version = ET.SubElement(root, f"{{{APP_NS}}}AppVersion")

    if family == "cell":
        application.text = "Cell"
        version.text = "12.0300"
    else:
        application.text = "Show"
        version.text = "12.0000"

    ET.register_namespace("", APP_NS)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def stamp_cell_workbook(payload: bytes) -> bytes:
    root = ET.fromstring(payload)
    if root.tag != f"{{{SPREADSHEET_NS}}}workbook":
        raise ValueError("xl/workbook.xml has an unexpected root")

    file_versions = root.findall(f"{{{SPREADSHEET_NS}}}fileVersion")
    if len(file_versions) > 1:
        raise ValueError("xl/workbook.xml contains duplicate producer markers")
    if file_versions:
        file_version = file_versions[0]
    else:
        file_version = ET.Element(f"{{{SPREADSHEET_NS}}}fileVersion")
        root.insert(0, file_version)
    file_version.set("appName", "HCell")
    file_version.set("lastEdited", "12.0")

    calc_properties = root.findall(f"{{{SPREADSHEET_NS}}}calcPr")
    if len(calc_properties) > 1:
        raise ValueError("xl/workbook.xml contains duplicate calculation properties")
    if calc_properties:
        calc = calc_properties[0]
    else:
        calc = ET.SubElement(root, f"{{{SPREADSHEET_NS}}}calcPr")
    marker = f"{{{HANCOM_SHEET_NS}}}hclCalcId"
    calc.set(marker, "904")

    ET.register_namespace("", SPREADSHEET_NS)
    ET.register_namespace("hs", HANCOM_SHEET_NS)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def canonicalize_internal_targets(root: ET.Element, source_part: str | None) -> None:
    source_parent = posixpath.dirname(source_part) if source_part else "."
    for relationship in root.findall(f"{{{PACKAGE_RELS_NS}}}Relationship"):
        if relationship.get("TargetMode") not in (None, "Internal"):
            continue
        target = relationship.get("Target", "")
        if not target.startswith("/"):
            continue
        if target.startswith("//") or target == "/":
            raise ValueError(f"ambiguous absolute relationship target: {target!r}")
        relationship.set(
            "Target", posixpath.relpath(target.removeprefix("/"), source_parent)
        )


def relationship_source_part(relationship_part: str) -> str:
    path = PurePosixPath(relationship_part)
    if path.parent.name != "_rels" or not path.name.endswith(".rels"):
        raise ValueError(f"unexpected relationship part path: {relationship_part!r}")
    return str(path.parent.parent / path.name.removesuffix(".rels"))


def normalize_part_relationships(payload: bytes, relationship_part: str) -> bytes:
    root = ET.fromstring(payload)
    if root.tag != f"{{{PACKAGE_RELS_NS}}}Relationships":
        raise ValueError(f"{relationship_part} has an unexpected root")
    canonicalize_internal_targets(root, relationship_source_part(relationship_part))
    ET.register_namespace("", PACKAGE_RELS_NS)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def normalize_root_relationships(payload: bytes, family: str) -> bytes:
    root = ET.fromstring(payload)
    if root.tag != f"{{{PACKAGE_RELS_NS}}}Relationships":
        raise ValueError("_rels/.rels has an unexpected root")

    for relationship in list(root.findall(f"{{{PACKAGE_RELS_NS}}}Relationship")):
        if relationship.get("Type") == CUSTOM_REL:
            root.remove(relationship)
    canonicalize_internal_targets(root, None)

    expected = {
        OFFICE_REL: "xl/workbook.xml" if family == "cell" else "ppt/presentation.xml",
        APP_REL: "docProps/app.xml",
    }
    found: set[str] = set()
    for relationship in root.findall(f"{{{PACKAGE_RELS_NS}}}Relationship"):
        relationship_type = relationship.get("Type")
        if relationship_type not in expected:
            continue
        if relationship_type in found:
            raise ValueError(f"duplicate root relationship type: {relationship_type!r}")
        if relationship.get("TargetMode") is not None:
            raise ValueError("main and application root relationships must be internal")
        target = relationship.get("Target", "").lstrip("/")
        if target != expected[relationship_type]:
            raise ValueError(f"unexpected root relationship target: {target!r}")
        relationship.set("Target", target)
        found.add(relationship_type)
    if found != set(expected):
        raise ValueError("_rels/.rels lacks the main or application relationship")

    ET.register_namespace("", PACKAGE_RELS_NS)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def declare_main_content_type(payload: bytes, family: str) -> bytes:
    root = ET.fromstring(payload)
    if root.tag != f"{{{CONTENT_TYPES_NS}}}Types":
        raise ValueError("[Content_Types].xml has an unexpected root")

    part_name = "/xl/workbook.xml" if family == "cell" else "/ppt/presentation.xml"
    content_type = CELL_MAIN_TYPE if family == "cell" else SHOW_MAIN_TYPE
    overrides = root.findall(f"{{{CONTENT_TYPES_NS}}}Override")
    for item in list(overrides):
        if item.get("PartName", "").lstrip("/") == CUSTOM_PROPERTIES_PART:
            root.remove(item)
            overrides.remove(item)

    defaults = root.findall(f"{{{CONTENT_TYPES_NS}}}Default")
    for extension, expected_type in (
        ("rels", "application/vnd.openxmlformats-package.relationships+xml"),
        ("xml", "application/xml"),
    ):
        matching_defaults = [
            item
            for item in defaults
            if item.get("Extension", "").lower() == extension
        ]
        if len(matching_defaults) > 1:
            raise ValueError(f"duplicate default content type: {extension}")
        if matching_defaults:
            default = matching_defaults[0]
            default.set("Extension", extension)
            default.set("ContentType", expected_type)
        else:
            default = ET.Element(
                f"{{{CONTENT_TYPES_NS}}}Default",
                {"Extension": extension, "ContentType": expected_type},
            )
            first_override = next(
                (
                    index
                    for index, child in enumerate(root)
                    if child.tag == f"{{{CONTENT_TYPES_NS}}}Override"
                ),
                len(root),
            )
            root.insert(first_override, default)

    matching = [item for item in overrides if item.get("PartName") == part_name]
    if len(matching) > 1:
        raise ValueError(f"duplicate main content-type override: {part_name}")
    if matching:
        matching[0].set("ContentType", content_type)
    else:
        ET.SubElement(
            root,
            f"{{{CONTENT_TYPES_NS}}}Override",
            {"PartName": part_name, "ContentType": content_type},
        )

    ET.register_namespace("", CONTENT_TYPES_NS)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def copy_zip_with_fingerprint(source: Path, destination: Path, family: str) -> None:
    required_main = "xl/workbook.xml" if family == "cell" else "ppt/presentation.xml"
    seen: set[str] = set()

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent, delete=False
    ) as temporary:
        temporary_path = Path(temporary.name)

    try:
        with zipfile.ZipFile(source, "r") as incoming, zipfile.ZipFile(
            temporary_path, "w", allowZip64=False
        ) as outgoing:
            for info in incoming.infolist():
                if info.filename in seen:
                    raise ValueError(f"duplicate OOXML entry: {info.filename}")
                seen.add(info.filename)
                if info.filename == CUSTOM_PROPERTIES_PART:
                    continue
                payload = incoming.read(info)
                if info.filename == "[Content_Types].xml":
                    payload = declare_main_content_type(payload, family)
                elif info.filename == "_rels/.rels":
                    payload = normalize_root_relationships(payload, family)
                elif info.filename.endswith(".rels"):
                    payload = normalize_part_relationships(payload, info.filename)
                elif info.filename == "docProps/app.xml":
                    payload = stamp_app_properties(payload, family)
                elif family == "cell" and info.filename == required_main:
                    payload = stamp_cell_workbook(payload)
                outgoing.writestr(info, payload)

        required = {"[Content_Types].xml", "_rels/.rels", "docProps/app.xml", required_main}
        missing = required - seen
        if missing:
            raise ValueError(f"native OOXML seed lacks required entries: {sorted(missing)}")

        temporary_path.replace(destination)
    except Exception:
        temporary_path.unlink(missing_ok=True)
        raise


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("family", choices=("cell", "show"))
    parser.add_argument(
        "source",
        type=Path,
        help="otherwise carrier-profile-compatible .xlsx or .pptx seed",
    )
    parser.add_argument("destination", type=Path, help="output .cell or .show carrier")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    expected_source = ".xlsx" if args.family == "cell" else ".pptx"
    expected_destination = ".cell" if args.family == "cell" else ".show"
    if args.source.suffix.lower() != expected_source:
        raise ValueError(f"{args.family} source must use {expected_source}")
    if args.destination.suffix.lower() != expected_destination:
        raise ValueError(f"{args.family} destination must use {expected_destination}")
    if args.source.resolve() == args.destination.resolve():
        raise ValueError("source and destination must differ")

    copy_zip_with_fingerprint(args.source, args.destination, args.family)
    shutil.copystat(args.source, args.destination)
    print(args.destination)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
