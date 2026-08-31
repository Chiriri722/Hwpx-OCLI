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

Verified on 2026-08-31 from Korean government publication pages. These files
are interoperability evidence, not format specifications, and are not
redistributed in this repository. The corpus contains two Cell and thirteen
Show attachments, plus one separately published XLSX. One Cell and eight Show
attachments fit the supported profiles. The other Cell and five Show
attachments are negative-boundary evidence. The exact attachment URLs, byte
lengths, and digests make the observations reproducible without treating a
filename as provenance.

### Supported-profile evidence

| Family | Official attachment | Bytes | SHA-256 | Publication | Observation |
|---|---|---:|---|---|---|
| Cell | [`직종별사업체노동력조사_조사표(2026상반기).cell`](https://www.moel.go.kr/local/seoul/common/downloadFile.do?file_seq=20260401245&bbs_seq=20260400662&bbs_id=LOCAL5) | 1,242,079 | `874f35e3091f12f322405357a603d66fc38179da99ad1fea80b9a9d999071c75` | [Seoul Regional Employment and Labor Administration](https://www.moel.go.kr/local/seoul/info/dataroom/view.do?bbs_seq=20260400662) | Cell 12.0300 profile |
| Paired XLSX | [`직종별사업체노동력조사_조사표(2026상반기).xlsx`](https://www.moel.go.kr/local/seoul/common/downloadFile.do?file_seq=20260401248&bbs_seq=20260400662&bbs_id=LOCAL5) | 1,282,392 | `e103a07de69a8521fcb66144a0326055440e3a866ca91090e708c915cadc1760` | same publication as the Cell sample | independent read oracle |
| Show | [`16-재난) 집중호우가 오면 이렇게 지켜요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=f186ad14e1e9a7179bd045dc54283c1f) | 1,693,635 | `60daebffd61afb16e2a9f284d08678134aa6a19cc3ccc5d7c3b23167165191a1` | [Gyeonggi Provincial Office of Education](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=1006126) | Show 12.0000 profile |
| Show | [`17-직업) 안전장비를 입고 안전하게 일해요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=c18e998cca038980bab6b0479dd8347a) | 6,097,986 | `01559ad550e0449de53faa9a9dd3e9741e55bcdbfac9a4e5fc50d809c01ce5b3` | same Gyeonggi publication | Show 12.0000 profile |
| Show | [`18-직업) 안전하게 배달해주세요(한쇼).show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=a49892ca6b68d7d1e60e030f27d93324) | 8,013,886 | `ebe0612ac63a809ac7d90f0c1a5606d3ac0e9dd23e1488c3e20b576bb0cb2e8d` | same Gyeonggi publication | Show 12.0000 profile |
| Show | [`21-응급) 앗 뜨거워! 조심하세요!.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=2631d6b37627bc059523fff5bcc9ccd7) | 18,550,489 | `3437797d2c8682c89576ae6719c81443cdbeb73a4e3f91a732b9dcf50b33c527` | [Gyeonggi kindergarten safety materials 19–21](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=1006128) | two slide-embedded GIF89a parts |
| Show | [`22-응급) 응급처치가 필요해요!.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=41e809a9e3780ccdb4fe31df16193673) | 20,136,036 | `4724a42570b41dcd6f5c6cd68e53e3e5b3442385132537159ec3a0ec5a938f37` | [Gyeonggi kindergarten safety materials 22–24](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=1006129) | one slide-embedded GIF89a part |
| Show | [`7-1)-138쪽 응급처치(영극을 통한 응급처치 중요성 방법 알기) 국어.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=d8fb450e0b44871422d4f64ce4f42f57) | 3,431,677 | `f0ca50f5bfa8f6a05bc35cdaad85eab282805054d25f9830b0a6729943c52301` | [Gyeonggi middle-school first-aid materials](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=989906) | Show 12.0000 profile |
| Show | [`7-2)-142쪽 응급처치(나폴레옹 원정에서 배우는 동상예방) 역사.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=4c3e5c10ef1d452a0e9fa809ea7ea664) | 534,888 | `4f20f68faf714a56a9c1e293f9ec0066d5177d3be87eef93557df3b756e7e8ab` | same middle-school publication | Show 12.0000 profile |
| Show | [`7-3)-143쪽 응급처치(응급처치 RICE) 체육.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=b787762fcbfcf36ac3f9581a6b603732) | 1,918,634 | `460476f51dc406526f2e16eb89e5fbf5f9cb9ffe59b846b2b8168354e8fbb708` | same middle-school publication | Show 12.0000 profile |

### Negative-boundary evidence

| Family | Official attachment | Bytes | SHA-256 | Publication | Rejected boundary |
|---|---|---:|---|---|---|
| Cell | [`2026 계약제교원 발령관리 자동화프로그램(초중고통합_20260420버전).cell`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=85ebce3d2aa97386b6c4a235ba825134) | 1,450,425 | `14e61d5db091195d7c17ec94db0f4bd8347014461d33842a2d72ff69480f3d74` | [Gyeonggi school-work automation materials](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?nttSn=2341820) | VBA project |
| Show | [`교직원 및 학부모 심폐소생술 등 응급처치교육(한쇼 버전).show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=26f76e6bb7c5e0d6c73793a2966740e3) | 13,160,523 | `6b27154d4701ba64f2f0a94961f119f84ffd7712940fde45064e5efcefd04355` | [Gyeonggi first-aid instructor materials](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=977134) | OLE embedding |
| Show | [`23-생명존중) 버려지는 강아지가 있어요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=a3dec2e98dc695c664af97c3c5f394f9) | 23,450,045 | `21d1ba7185cc7dfc95f1a3aebf1d589dfe519e7a523b49ff142c322e7365cc08` | [Gyeonggi kindergarten safety materials 22–24](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=1006129) | external HTTP hyperlink |
| Show | [`24-생명존중) 소중한 바다를 지켜야 해요.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=9cb8b358afb8181851e6a30d59a1c010) | 9,249,456 | `efaeeb601cde635fa39fa5ec31b347d7ffe4ff44919d9735491a4dc555a5ae1c` | same kindergarten publication | Microsoft media relationship |
| Show | [`2-1)-26쪽 교통안(자전거 탑승 안전) 국어.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=74563d41d9414fc136cea07063851a5a) | 2,204,711 | `eaab715e0482c33b6b24bd9a04fbac488a36f2890d5ff8d8bb492defacfc0b4f` | [Gyeonggi middle-school traffic-safety materials](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=989895) | video relationship |
| Show | [`7-7) 4과학-응급처치-화상을 입었을 경우 대처법 알아보기-동영상 삭제한 파일.show`](https://www.goe.go.kr/goe/na/ntt/comm/nttFileDownload.do?fileKey=fdfe5d8ec830e21865788e8063abad7e) | 20,781,060 | `b8cee0bbe20e08260ea89bd5a678692f76110e439437d4594342ea21997aea31` | [Gyeonggi elementary first-aid materials](https://www.goe.go.kr/goe/na/ntt/selectNttInfo.do?mi=10139&nttSn=990170) | Microsoft media relationship |

The separately published Cell and XLSX attachments are not byte-identical and
do not have the same package topology. They are therefore an independent read
oracle, not evidence that renaming every Cell file is a general conversion.
The supported application-profile markers classify packages; they do not
authenticate a producer. The exact observations and rejection policy are
recorded in
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
