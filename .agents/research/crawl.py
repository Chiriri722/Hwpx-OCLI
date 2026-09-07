"""Batch crawler for Hancom Office format research (crawl4ai).

Usage: python crawl.py <batch-name> <url> [<url> ...]
Writes one markdown file per URL under .agents/research/out/<batch-name>/.
"""

import asyncio
import hashlib
import pathlib
import re
import sys

from crawl4ai import AsyncWebCrawler, BrowserConfig, CacheMode, CrawlerRunConfig

OUT_ROOT = pathlib.Path(__file__).resolve().parent / "out"


def slug(url: str) -> str:
    base = re.sub(r"[^a-zA-Z0-9]+", "-", url.split("://", 1)[-1]).strip("-")[:80]
    return f"{base}-{hashlib.sha256(url.encode()).hexdigest()[:8]}"


async def main() -> int:
    batch, urls = sys.argv[1], sys.argv[2:]
    if not urls:
        print("no urls given", file=sys.stderr)
        return 2
    out_dir = OUT_ROOT / batch
    out_dir.mkdir(parents=True, exist_ok=True)

    browser = BrowserConfig(headless=True, verbose=False)
    run = CrawlerRunConfig(cache_mode=CacheMode.BYPASS, page_timeout=60000)

    async with AsyncWebCrawler(config=browser) as crawler:
        results = await crawler.arun_many(urls=urls, config=run)
        for res in results:
            url = getattr(res, "url", "?")
            if not getattr(res, "success", False):
                print(f"FAIL {url}: {getattr(res, 'error_message', 'unknown')}")
                continue
            md = getattr(res, "markdown", None)
            text = getattr(md, "raw_markdown", None) or str(md or "")
            path = out_dir / f"{slug(url)}.md"
            path.write_text(f"<!-- source: {url} -->\n\n{text}", encoding="utf-8")
            print(f"OK   {url} -> {path.name} ({len(text)} chars)")
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
