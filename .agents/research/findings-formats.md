# Hancom Office 포맷·도구 조사 결과

조사일 2026-08-28. 수집 도구: crawl4ai(`.agents/research/crawl.py`), GitHub REST API,
Apache Tika mimetype 레지스트리, HTTP HEAD 검증.
원문은 `.agents/research/out/<batch>/*.md`에 보존한다.

증거 등급: **CONFIRMED** = 1차 출처(한컴 공식/표준/API 응답)로 직접 확인,
**LIKELY** = 정황 근거만 있음, **UNKNOWN** = 확인 실패.

---

## 1. 확장자 지도 (CONFIRMED)

출처: 한컴 공식 한컴오피스 뷰어 제품 페이지 "지원하는 포맷"
<https://www.hancom.com/product/office/officeViewer>
(로컬 사본 `out/viewer/www-hancom-com-product-office-officeViewer-93c8b9a9.md` 47–51행)

```
한컴오피스 문서(*.hwp; *hwpx; *.hwt; *.hml; *.hwdt; *.hwpx; *.nxl; *.hcdt; *.hpt; *.hsdt)
스프레드시트 문서(*.cell; *.hcdt; *.nxl; *.nxt *.xls; *.xlsx; *.xlsm; *.ods)
프레젠테이션 문서(*.show;*.pptx;*.ppt;*.potx;*.pps;*.ppsx;*.odp)
```

| 앱 | 주 확장자 | 부 확장자 | 컨테이너 | 근거 |
|---|---|---|---|---|
| 한글 | `.hwp` | `.hwt`(서식), `.hwdt` | CFB/OLE2 (5.x) | 공식 스펙 |
| 한글 | `.hwpx` (2010+), `.owpml` (2018+) | — | ZIP + OWPML XML | KS X 6101 |
| 한글 | `.hml` | — | 단일 XML (HWPML) | 공식 스펙 |
| 한셀 | `.cell` (2010+) | `.nxl`(넥셀 레거시), `.nxt`, `.hcdt` | **UNKNOWN** | 뷰어 페이지 |
| 한쇼 | `.show` | `.hpt`, `.hsdt`, `.htheme` | **UNKNOWN** | 뷰어 페이지 |

주의: 한컴 뷰어 페이지 원문 자체가 `*.hwpx`를 두 번 쓰고 `.hcdt`를 두 그룹에 넣는 등
부정확하다. `.hpt`/`.hsdt`는 한쇼 계열로 보이나 페이지에서는 한글 그룹에 있다.
정확한 소속은 실제 한컴오피스 저장 대화상자로 재확인해야 한다.

### `.cellx` / `.showx` 는 존재하지 않는다 (CONFIRMED-부정)

한컴 공식 페이지, 뷰어 지원 포맷, Apache Tika 레지스트리, GitHub 전체 검색에서
`.cellx`/`.showx` 언급이 0건이다. OWPML(KS X 6101)은 표준명이
"개방형 **워드프로세서** 마크업 언어"로 워드프로세서 전용이며,
스프레드시트·프레젠테이션용 개방 표준은 제정된 바 없다.
→ **한셀·한쇼에는 HWPX에 대응하는 개방형 XML 포맷이 없다.**

### 한셀/한쇼 이력 (CONFIRMED)

`out/hancell-show/namu-wiki-...한셀...md` 80–81행: 한/셀 2010에서 이름이 한셀로 바뀌며
기본 확장자가 `*.nxl` → `*.cell`로 변경, 한/셀 2014에서 **확장자는 그대로지만 파일 구조가
다시 변경**. → `.cell`은 최소 2개의 비호환 내부 구조 세대를 가진다.

---

## 2. 공식 스펙 문서 (CONFIRMED — HTTP 200 / application/pdf 검증 완료)

현재 공식 출처는 <https://www.store.hancom.com/support/downloadCenter/hwpOwpml>이다.
`.hwp`와 `.pdf` 두 형식으로 제공한다. 2026-08-28에 PDF 5개를 GET으로 다시 받아
HTTP 상태·미디어 타입·`%PDF-` magic·바이트 수·SHA-256을 검증했으며, 정확한 URL과
해시는 [`docs/spec-sources.md`](../../docs/spec-sources.md)에 고정했다.

| 문서 | 리비전 | PDF 크기 | 상태 |
|---|---|---|---|
| 한글문서파일형식 5.0 | **1.3** | 830,986 B | 200 OK |
| 한글문서파일형식 3.0 / HWPML | 1.2 | 905,504 B | 200 OK |
| 배포용 문서 형식 | 1.2 | 187,952 B | 200 OK |
| 수식 형식 | **1.3** | 444,480 B | 200 OK |
| 차트 형식 | 1.2 | 437,034 B | 200 OK |

URL 형태: `https://cdn.hancom.com/link/docs/한글문서파일형식_5.0_revision1.3.pdf` (퍼센트 인코딩).

공개 이력: 2010-06-29 HWP·HWPML 공개, 2014-10 5.0 스펙 보완 + 배포용/수식/차트 추가.

### 필수 저작권·표기 의무 (CONFIRMED — 법적 요구사항)

같은 페이지 "HWP 문서 파일 형식에 대한 사용권 및 저작권" 절 원문:

> 반드시 개발 결과물에 "본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여
> 개발하였습니다."라고 제품 내 **유저인터페이스, 매뉴얼, 도움말 및 소스에 모두**
> 기재하여야 하며 제품이 이러한 구성물이 없을 시에는 존재하는 구성물에만 기재합니다.

추가 조건: 스펙 문서 재배포는 무수정 원본/사본으로 제한되고 최신 버전을 포함해야 한다.
한컴은 스펙 파생 결과물로 배타적 권리를 취득해 한컴을 상대로 행사하는 자에게
권리행사할 수 있다고 명시한다. 정확성·진실성은 보증하지 않는다.

→ 조사 시점에는 `plugins/hwpx`에 이 문구가 없었다(`grep` 결과 0건). 2026-08-28
P0-T0-1에서 `--info`, `--help`, 플러그인 README, 플러그인 NOTICE에 반영했다.

---

## 3. KS X 6101 (OWPML) (CONFIRMED)

- 표준번호 **KS X 6101:2011**, 표준명 "개방형 워드프로세서 마크업 언어 (OWPML) 문서 구조"
- 제정 2011-12-30. HWPML의 국가표준 버전. 2010년부터 약 2년간 국내 문서표준화위원회 진행.
- 열람: KSSN <https://www.kssn.net/search/stddetail.do?itemNo=K001010119985> /
  e나라표준인증 `standard.go.kr ... ksNo=KSX6101`
- 무료 PDF 다운로드는 불가. KS 원문보기 뷰어 또는 구매 필요.
- 공개된 단독 XSD 스키마 파일은 없음(CONFIRMED). 스키마가 표준 본문 부속서에 포함될
  가능성은 LIKELY이며 미확인.

---

## 4. 한셀/한쇼 내부 구조 (UNKNOWN — 이 계획의 최대 리스크)

- 공개된 스펙·스키마·리버스엔지니어링 문서 **0건**.
- Apache Tika `tika-mimetypes.xml`(333KB 전체 검사)에는 `application/x-hwp`,
  `x-hwp-v5`, `application/hwp+zip`(`.hwpx`)만 등록. `.cell`/`.show` 항목 없음.
  (`.agents/research/tika-hancom.txt` 4260–4279행)
- LibreOffice에는 HWP용 `hwpfilter`만 있고 `.cell`/`.show` import 필터 없음.
- GitHub 코드 검색 `total_count=0` — 5개 질의 전부 0건
  (`hancell parser`, `hanshow parser`, `"한셀" 파일 형식 파서`, `hcell format`, `nxl nexcel`).
  결과: `.agents/research/gh-cell-show.txt`
- 컨테이너 타입: **UNKNOWN**. 동시대 한컴 코드베이스(HWP 5.x)가 CFB/OLE2이므로
  CFB일 가능성이 LIKELY이나 ZIP 가능성도 배제 못 함. 실제 표본으로 매직바이트
  (`D0 CF 11 E0 A1 B1 1A E1` vs `50 4B 03 04`) 확인이 필수.

---

## 5. 한컴 공식 SDK (CONFIRMED — 단, 사용 불가)

<https://www.hancom.com/product/sdk> 계열 페이지 기준 목록:
한컴오피스 SDK / 한글 SDK / **한셀 SDK** / 한PDF SDK / 한컴 AI SDK / 한컴 오스 SDK.
**한쇼 SDK는 목록에 없다.**

한셀 SDK(`/product/sdk/hancellSdk`)는 "함수, 수식 계산, 데이터 분석 등 스프레드시트의
기능을 고객사의 제품과 서비스에 탑재"하는 **계산엔진 임베딩 SDK**이며 420여 개 함수군을
제공한다. 도입 절차 1단계가 "**한셀 SDK 구매**"인 상용 제품이다.
→ `.cell` 파일 포맷 파서가 아니고, 유료·비공개이므로 MIT 플러그인에 사용할 수 없다.

한컴오피스 SDK는 UX 프레임 화이트라벨 SDK(적용 사례: 대만 KDAN Office)로 역시 무관.

---

## 6. 오픈소스 선행 기술 (CONFIRMED via GitHub API)

전체 조사 결과: `.agents/research/gh-survey.txt`

| 프로젝트 | 언어 | 라이선스 | ★ | 읽기 | **쓰기** | 대상 |
|---|---|---|---|---|---|---|
| `edwardkim/rhwp` | Rust | MIT | 3753 | O | ? | hwp, hwpx |
| `neolord0/hwplib` | Java | Apache-2.0 | — | O | **O** | hwp (암호화 포함) |
| `neolord0/hwpxlib` | Java | Apache-2.0 | 181 | O | **O** | hwpx (암호화 포함) |
| `hancom-io/hwpx-owpml-model` | C++ | Apache-2.0 | 37 | O | ? | hwpx (공식) |
| `hancom-io/dvc` | C++ | — | 9 | 검증기 | — | hwpx (공식 검증기) |
| `hancom-io/hwpx-contents-extract` | Java | — | 3 | O | X | hwpx (공식) |
| `hahnlee/hwp.js` | TS | Apache-2.0 | 1305 | O | X | hwp |
| `mete0r/pyhwp` | Python | NOASSERTION | 301 | O | X | hwp |
| `openhwp/openhwp` | Rust | MIT | 92 | O | ? | hwp |
| `Indosaram/hwpers` | Rust | MIT | 193 | O | ? | hwp |
| `airmang/python-hwpx` | Python | Apache-2.0 | 106 | O | ? | hwpx |

핵심 관찰:
- **쓰기(WRITE)를 확실히 지원하는 것은 `hwplib`/`hwpxlib` 두 Java 라이브러리뿐**이며
  둘 다 Apache-2.0으로 라이선스가 호환된다. 다만 JVM 의존이 생긴다.
  (`hwplib`는 CFB 파싱에 Apache POI 사용, Maven Central 게시)
- `rhwp` v0.8.4는 Rust·MIT·활발(3753★, 40+ 기여자, ROADMAP.md 존재)하며 현재
  플러그인이 이미 `.hwp` 변환기로 사용 중. **한셀/한쇼 언급은 없다.**
- `hancom-io/dvc`는 공식 HWPX 검증기로 CI 게이트 후보다.
- **한셀/한쇼용 오픈소스는 전무하다.**

---

## 7. 확인하지 못한 항목

1. `.cell`/`.show` 내부 스트림 구조 및 컨테이너 타입 — 실제 표본 필요.
2. `.cell` 구조가 2010/2014에 각각 어떻게 바뀌었는지.
3. KS X 6101 본문에 완전한 XSD가 포함되는지 (유료 열람 필요).
4. `.hcdt`/`.hsdt`/`.hpt`/`.nxt`/`.htheme`의 정확한 앱 소속과 내부 구조.
5. `rhwp`의 정확한 CLI 표면과 `.hwpx` 쓰기 가능 여부 (README 111KB 미완독).
6. 한글/한셀/한쇼의 Windows COM 자동화 객체 존재 여부 (`HWPFrame.HwpObject` 계열).
