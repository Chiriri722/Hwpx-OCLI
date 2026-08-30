#!/usr/bin/env python3
"""Generate a small, strict HWPX package for format-handler smoke tests."""

from __future__ import annotations

import argparse
from html import escape
from pathlib import Path
import zipfile


VERSION = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" '
    'tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" '
    'buildNumber="0" xmlVersion="1.4" application="OfficeCLI" appVersion="0.1.0"/>'
)
MANIFEST = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<odf:manifest '
    'xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>'
)
CONTAINER = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<ocf:container '
    'xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container">'
    '<ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" '
    'media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>'
)
CONTENT_HPF = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<opf:package xmlns:opf="http://www.idpf.org/2007/opf/">'
    '<opf:manifest>'
    '<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>'
    '<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>'
    '</opf:manifest><opf:spine><opf:itemref idref="header"/>'
    '<opf:itemref idref="section0"/></opf:spine></opf:package>'
)
HEADER = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    '<hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" '
    'version="1.4" secCnt="1"><hh:refList>'
    '<hh:charProperties itemCnt="1"><hh:charPr id="0" height="1000"/>'
    '</hh:charProperties><hh:paraProperties itemCnt="1">'
    '<hh:paraPr id="0"/></hh:paraProperties></hh:refList></hh:head>'
)


def zip_info(name: str, compression: int) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = compression
    info.external_attr = 0o100644 << 16
    return info


def section(first: str, second: str) -> str:
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" '
        'xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">'
        '<hp:p id="7" paraPrIDRef="0"><hp:run charPrIDRef="0">'
        f'<hp:t>{escape(first)}</hp:t></hp:run><hp:linesegarray>'
        '<hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p>'
        '<hp:p id="8" paraPrIDRef="0"><hp:run charPrIDRef="0">'
        f'<hp:t>{escape(second)}</hp:t></hp:run><hp:linesegarray>'
        '<hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray></hp:p>'
        '</hs:sec>'
    )


def write_fixture(path: Path, first: str, second: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            zip_info("mimetype", zipfile.ZIP_STORED),
            "application/hwp+zip",
        )
        for name, body in (
            ("version.xml", VERSION),
            ("META-INF/manifest.xml", MANIFEST),
            ("META-INF/container.xml", CONTAINER),
            ("Contents/content.hpf", CONTENT_HPF),
            ("Contents/header.xml", HEADER),
            ("Contents/section0.xml", section(first, second)),
        ):
            archive.writestr(zip_info(name, zipfile.ZIP_DEFLATED), body)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--text", default="before")
    parser.add_argument("--second-text", default="second")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    output = args.output.expanduser().resolve()
    if output.exists() and not args.force:
        parser.error(f"output already exists (pass --force to replace it): {output}")
    write_fixture(output, args.text, args.second_text)
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
