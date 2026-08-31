# Hancom public specifications and interoperability sources

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

## Public Cell and Show interoperability samples

Verified on 2026-08-30 from Korean government publication pages. These files
are interoperability evidence, not format specifications, and are not
redistributed in this repository. The exact attachment URLs, byte lengths, and
digests make the observations reproducible without treating a filename as
provenance.

| Family | Official attachment | Bytes | SHA-256 | Publication |
|---|---|---:|---|---|
| Cell | [`직종별사업체노동력조사_조사표(2026상반기).cell`](https://www.moel.go.kr/local/seoul/common/downloadFile.do?file_seq=20260401245&bbs_seq=20260400662&bbs_id=LOCAL5) | 1,242,079 | `874f35e3091f12f322405357a603d66fc38179da99ad1fea80b9a9d999071c75` | [Seoul Regional Employment and Labor Administration](https://www.moel.go.kr/local/seoul/info/dataroom/view.do?bbs_seq=20260400662) |
| Paired XLSX | [`직종별사업체노동력조사_조사표(2026상반기).xlsx`](https://www.moel.go.kr/local/seoul/common/downloadFile.do?file_seq=20260401248&bbs_seq=20260400662&bbs_id=LOCAL5) | 1,282,392 | `e103a07de69a8521fcb66144a0326055440e3a866ca91090e708c915cadc1760` | same publication as the Cell sample |
| Show | [`16-재난) 집중호우가 오면 이렇게 지켜요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=f186ad14e1e9a7179bd045dc54283c1f) | 1,693,635 | `60daebffd61afb16e2a9f284d08678134aa6a19cc3ccc5d7c3b23167165191a1` | [Gyeonggi Provincial Office of Education](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=1006126) |
| Show | [`17-직업) 안전장비를 입고 안전하게 일해요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=c18e998cca038980bab6b0479dd8347a) | 6,097,986 | `01559ad550e0449de53faa9a9dd3e9741e55bcdbfac9a4e5fc50d809c01ce5b3` | same Gyeonggi publication |
| Show | [`18-직업) 안전하게 배달해주세요(한쇼).show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=a49892ca6b68d7d1e60e030f27d93324) | 8,013,886 | `ebe0612ac63a809ac7d90f0c1a5606d3ac0e9dd23e1488c3e20b576bb0cb2e8d` | same Gyeonggi publication |

The separately published Cell and XLSX attachments are not byte-identical and
do not have the same package topology. They are therefore an independent read
oracle, not evidence that renaming every Cell file is a general conversion.
The supported application-profile markers (classification, not producer
authentication) and the exact observations drawn from these
samples are recorded in
[ADR-0016](adr/0016-hancom-v12-ooxml-carrier-bridge.md).

## Verification procedure

For each specification row, download the PDF URL to a temporary directory and
reject a response unless its status is 200, its media type is
`application/pdf`, and its first five bytes are `%PDF-`. For each
interoperability row, download the attachment URL from its linked publication
page and require status 200; do not infer a media type from the filename because
the government download endpoints use generic response headers. In both cases,
compare the exact byte length and lowercase SHA-256 above, then delete the
temporary files after verification.

On PowerShell, the digest command is:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath <downloaded-file>
```

If Hancom publishes a newer revision, add it as a new row and keep the old row
for provenance; do not silently replace a digest under an existing revision.

## Required attribution

Hancom's official page requires products derived from the public HWP documents
to carry the following notice in their available user interface, manual, help,
and source surfaces:

> 본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.

This repository places that notice in the plugin's `--info` description,
`--help`, `plugins/hancom/README.md`, and `plugins/hancom/NOTICE`.
