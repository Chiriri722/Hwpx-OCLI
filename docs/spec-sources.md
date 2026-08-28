# Hancom HWP public specification sources

Verified on 2026-08-28 against Hancom's official
[HWP/OWPML format page](https://www.hancom.com/support/downloadCenter/hwpOwpml).
Each URL returned HTTP 200 with `Content-Type: application/pdf`, preserved the
same final URL (no redirect), and began with the `%PDF-` signature. The PDF
bytes are intentionally not stored or redistributed in this repository.

| Document | Official filename | Revision | Bytes | SHA-256 | Official URL |
|---|---|---:|---:|---|---|
| HWP 5.0 | `한글문서파일형식_5.0_revision1.3.pdf` | 1.3 | 830,986 | `1d1da9e6fe22563ae2c5285bbbfc6762974fb7f002278084cbc11e5266bdc782` | [download](https://cdn.hancom.com/link/docs/%ED%95%9C%EA%B8%80%EB%AC%B8%EC%84%9C%ED%8C%8C%EC%9D%BC%ED%98%95%EC%8B%9D_5.0_revision1.3.pdf) |
| HWP 3.0 / HWPML | `한글문서파일형식3.0_HWPML_revision1.2.pdf` | 1.2 | 905,504 | `1b5ce665fe0b75967d85f965a4b32737487935974b8e29120537e71cacefca87` | [download](https://cdn.hancom.com/link/docs/%ED%95%9C%EA%B8%80%EB%AC%B8%EC%84%9C%ED%8C%8C%EC%9D%BC%ED%98%95%EC%8B%9D3.0_HWPML_revision1.2.pdf) |
| Distribution document | `한글문서파일형식_배포용문서_revision1.2.pdf` | 1.2 | 187,952 | `79cc8fb42c914f59e942e96c71926f11634799f37afe8ff924e16c5d1609a4bd` | [download](https://cdn.hancom.com/link/docs/%ED%95%9C%EA%B8%80%EB%AC%B8%EC%84%9C%ED%8C%8C%EC%9D%BC%ED%98%95%EC%8B%9D_%EB%B0%B0%ED%8F%AC%EC%9A%A9%EB%AC%B8%EC%84%9C_revision1.2.pdf) |
| Equation | `한글문서파일형식_수식_revision1.3.pdf` | 1.3 | 444,480 | `5877de457da2a182515b903eed60b42efe9ece49f6b91f2c0ecb735741457e23` | [download](https://cdn.hancom.com/link/docs/%ED%95%9C%EA%B8%80%EB%AC%B8%EC%84%9C%ED%8C%8C%EC%9D%BC%ED%98%95%EC%8B%9D_%EC%88%98%EC%8B%9D_revision1.3.pdf) |
| Chart | `한글문서파일형식_차트_revision1.2.pdf` | 1.2 | 437,034 | `e014db3e4b55bc57d93b3aba0b186151b3487575e3a6397a2983715b43beeeb1` | [download](https://cdn.hancom.com/link/docs/%ED%95%9C%EA%B8%80%EB%AC%B8%EC%84%9C%ED%8C%8C%EC%9D%BC%ED%98%95%EC%8B%9D_%EC%B0%A8%ED%8A%B8_revision1.2.pdf) |

## Verification procedure

Download each URL to a temporary directory, reject a response unless its status
is 200, its media type is `application/pdf`, and its first five bytes are
`%PDF-`, then compare the byte length and lowercase SHA-256 above. Delete the
temporary PDFs after verification.

On PowerShell, the digest command is:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath <downloaded-pdf>
```

If Hancom publishes a newer revision, add it as a new row and keep the old row
for provenance; do not silently replace a digest under an existing revision.

## Required attribution

Hancom's official page requires products derived from the public HWP documents
to carry the following notice in their available user interface, manual, help,
and source surfaces:

> 본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.

This repository places that notice in the plugin's `--info` description,
`--help`, `plugins/hwpx/README.md`, and `plugins/hwpx/NOTICE`.
