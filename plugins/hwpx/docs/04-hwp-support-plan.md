# `.hwp` (바이너리) 지원 계획

작성: 2026-08-01. 최종 갱신: 2026-08-13. 근거는 전부 실측이다.

> **진행 상태**: H2(포맷 판별), H3(rhwp 브리지), H4(쌍 회귀), H5(문서화)
> 완료. 새 Linux/Windows CI의 첫 결과 뒤 H1(discovery)만 남았다.

## 0. 결론 먼저

**RHWP를 HWP → HWPX 변환기로 붙인다.** 우리 파서는 손대지 않는다.

전 경로를 실제로 돌려봤고 통했다:

```
src.hwp  ──[rhwp export-hwpx]──▶  out.hwpx  ──[우리 플러그인]──▶  out.docx
```

| 단계 | 결과 |
|---|---|
| HWP → HWPX (rhwp v0.8.2) | 체크박스 8, 누름틀 4, 표 10 **전부 보존** |
| HWPX → BatchItem (우리) | 207개 항목, `plugins lint` 미지 prop **0** |
| BatchItem → docx (officecli) | `validate` 통과, 폼필드 checkbox 8 + text 3, 표 5 |

HWP 파일 4건 전부 같은 결과였다. 새로 쓸 파싱 코드가 **없다.**

2026-08-13에 RHWP v0.8.4 공식 배포물로 계약을 다시 확인했다. 공식 HWP5
표본 3종과 HWP3 표본 1종이 새 브리지에서 각각 19/712/48/467 JSONL 행으로
성공했다. 기존 v0.8.2 실문서 보존 결과는 역사적 기준선으로 유지한다.

## 1. 조사 결과

### 1-1. RHWP가 이 생태계의 단일 상류다

| 항목 | RHWP | hop |
|---|---|---|
| 저장소 | `edwardkim/rhwp` | `golbin/hop` |
| 라이선스 | **MIT** | MIT |
| 언어 | **Rust** | TypeScript |
| 평가 기준 | HWP/HWPX 읽기·쓰기 및 폼 보존 실측 | RHWP를 submodule로 사용하는 UI |
| HWP 파서 | **자체 구현** | **없음 — RHWP를 git submodule로 씀** |

`golbin/hop`의 `.gitmodules`:

```
[submodule "third_party/rhwp"]
	path = third_party/rhwp
	url = https://github.com/edwardkim/rhwp.git
```

즉 hop은 RHWP 위에 얹은 UI다. **HWP 역공학 자산은 RHWP 하나로 수렴한다.**
두 프로젝트를 따로 평가할 필요가 없다.

### 1-2. RHWP의 HWP 파서 구성

`src/parser/` (합계 약 250KB Rust):

| 파일 | 크기 | 역할 |
|---|---|---|
| `cfb_reader.rs` | 58KB | HWP 5.0 OLE(CFB) 컨테이너 |
| `doc_info.rs` | 54KB | DocInfo 스트림 (글꼴·글자모양·문단모양·테두리) |
| `control.rs` | 41KB | 컨트롤 (표·그림·**폼 컨트롤**·필드) |
| `body_text.rs` | 34KB | BodyText 섹션, 문단 레코드 |
| `crypto.rs` | 18KB | 암호 걸린 HWP |
| `tags.rs` / `record.rs` | 22KB | HWPTAG 상수, 레코드 구조 |
| `hwp3/` | dir | HWP 3.0 (구형 포맷) |
| `hwpx/`, `hml/` | dir | HWPX, HWPML |

핵심 의존성: `cfb 0.14`(OLE), `flate2`(zlib), `byteorder`, `encoding_rs`, `codepage`.
HWP 5.0은 CFB 컨테이너 + zlib 압축 레코드 스트림이라 이 조합이 정확히 맞는다.

**폼 컨트롤을 다룬다** — 이게 결정적이다:

```rust
// src/parser/control.rs
tags::FIELD_CLICKHERE => FieldType::ClickHere,   // 누름틀
b"tbc+" => FormType::CheckBox,                    // 체크박스
b"tbp+" => FormType::PushButton,
b"boc+" => FormType::ComboBox,
b"tbr+" => FormType::RadioButton,
// src/parser/tags.rs
pub const HWPTAG_FORM_OBJECT: u16 = HWPTAG_BEGIN + 75;
pub const CTRL_FORM: u32 = ctrl_id(b"form");
pub const FIELD_CLICKHERE: u32 = ctrl_id(b"%clk");
```

### 1-3. RHWP는 HWPX 직렬화기를 갖고 있다

`src/serializer/hwpx/` 디렉터리와 CLI 서브커맨드가 있다:

```
rhwp export-hwpx <입력.hwp|입력.hwpx> [출력.hwpx] [--verify] [--verify-pages]
```

다른 서브커맨드: `export-text`, `export-markdown`, `export-tables`,
`export-structure`, `export-svg`, `export-png`, `export-pdf`, `export-hml`,
`convert`, `hwpx-roundtrip`. `--verify`는 변환 전후 IR을 비교한다.

라이브러리 모듈이 모두 `pub`이다(`pub mod parser`, `pub mod serializer`).
즉 in-process 호출도 가능하다.

### 1-4. 배포 형태

GitHub Releases v0.8.2에 4개 플랫폼 프리빌트 + 체크섬:

```
5.4MB  rhwp-v0.8.2-macos-aarch64.tar.gz
5.8MB  rhwp-v0.8.2-macos-x86_64.tar.gz
6.0MB  rhwp-v0.8.2-linux-x86_64.tar.gz
5.9MB  rhwp-v0.8.2-windows-x86_64.zip
       SHA256SUMS.txt
```

**officecli 자신과 같은 배포 모델이다.** 우리가 이미
`scripts/verify-roundtrip.sh --download`에서 officecli를 그렇게 받고 있다.
crates.io·npm에는 없다.

최신 재검증 기준은 v0.8.4다. macOS aarch64 자산
`rhwp-v0.8.4-macos-aarch64.tar.gz`의 SHA-256
`6a5e6a7104a2ce40fd4235d1c95cc86b0291652f30f6d1bf1efd3708419ac176`이
공식 `SHA256SUMS.txt`와 일치했다. Linux x86_64, Windows x86_64,
macOS x86_64 자산도 릴리스에 함께 제공된다.

### 1-5. `unhwp` 재평가 — 내가 전에 틀렸다

`docs/01-protocol-contract.md` ADR-2에서 이렇게 적었다:

> `unhwp`의 출력(Markdown/text/JSON)은 런 단위 서식·셀 병합·색상을 표현하지 못한다.

**이건 사실이 아니다.** 크레이트 **설명문**("extracting HWP/HWPX documents into
structured Markdown")만 보고 판단했고, `src/model/`을 열어보지 않았다.
실제로는 구조화된 중간표현을 노출한다:

```rust
pub use model::Document;
pub fn parse_file(path) -> Result<Document>
// model: Document / Section / Block / Paragraph / TextRun / InlineContent
//        TextStyle{bold, italic, underline, strikethrough, superscript,
//                  subscript, font_name, font_size, color, background_color}
//        ParagraphStyle{heading_level, alignment, list_style, indent_level,
//                       line_spacing, space_before, space_after}
//        Table / TableRow / TableCell{content, rowspan, colspan, alignment,
//                                     vertical_alignment, background_color}
```

이 프로젝트에서 "설명만 보고 판단해서 틀린" 사례가 이번이 다섯 번째다
(취소선 `type`, 폰트 인덱스, `vmerge` enum, 그림 크기 단위, 그리고 이것).
ADR-2는 H5에서 정정했다(6절).

**그럼에도 `unhwp`를 HWP 경로에 쓰지 않는다.** 실측 이유:

`unhwp`에는 폼 컨트롤 처리가 **아예 없다.** 코드 전수 검색에서
`checkbox`/`checkbtn`/`clickhere`/`formfield`/`radio`/`combobox` 어느 것도
HWP5 경로에 없다. `src/hwp5/bodytext.rs`는 컨트롤 문자를 그냥 건너뛴다:

```
/// - Inline controls (8 WCHARs): 0x04, 0x05-0x08, ...
// === INLINE controls (size = 8 WCHARs) - skip 14 more bytes after control ===
```

즉 체크박스 8개와 누름틀 4개가 통째로 사라진다. **이 프로젝트가 시작된 이유가
"체크박스 조작이 힘들다"였으므로 이건 치명적이다.**

텍스트·표·서식만 보면 `unhwp`는 우리 HWPX 경로와 대등하다. 같은 문서의
HWP판(unhwp)과 HWPX판(우리)을 문자 단위로 대조한 결과:

| 문서 | HWP(unhwp) | HWPX(우리) | 차이 |
|---|---|---|---|
| 모집안내문 | 1,093자 | 1,095자 | 우리가 +2 (보존한 PUA 문자) |
| AI 제안서 | 259자 | 259자 | 없음 |

그러니 "텍스트만 필요한" 용도라면 `unhwp`도 충분하다. 우리 용도는 아니다.

### 1-6. `hwp` 크레이트 (hahnlee/hwp-rs)

Apache-2.0, "낮은 수준의 hwp 파서", 다운로드 4,153.
**최근 업데이트 2022-11-11** — 4년 가까이 멈췄다. 후보에서 제외한다.

## 2. 통합 방식 비교

### A1. `rhwp` 바이너리에 셸아웃 — **권장**

```
dump <src.hwp>
  → 임시 디렉터리에 rhwp export-hwpx src.hwp tmp.hwpx
  → 기존 OWPML 파서로 tmp.hwpx 읽기
  → 기존 emitter로 JSONL
```

**장점**

- **새 파싱 코드 0줄.** 우리가 이미 지원하는 모든 기능(체크박스·누름틀·중첩표·
  병합·내어쓰기·서식)이 그대로 적용된다. HWPX 경로를 개선하면 HWP도 같이 좋아진다.
- 현재 플러그인 바이너리의 작은 배포 형태를 유지한다.
- **프로토콜이 명시적으로 허용하는 패턴이다.** §5.1:
  > Main sets the `OFFICECLI_BIN` environment variable ... so plugins that
  > produce an intermediate `.docx` (**e.g. via an external converter**) can
  > shell out
- rhwp를 독립적으로 업그레이드할 수 있다. 우리를 다시 빌드할 필요가 없다.
- rhwp가 없으면 **exit 3**(`Feature unsupported in this build`)로 정직하게
  실패한다. 프로토콜 §6.5에 정확히 이 용도의 코드가 있다.
- HWP 3.0과 HWPML도 rhwp가 처리하므로 확장자를 늘리기 쉽다.

**단점**

- 런타임 의존성. 사용자가 rhwp를 설치해야 `.hwp`가 동작한다.
- 프로세스 생성 + 임시 파일. dump는 이미 일회성 실행이라 영향은 작다.
- 변환 단계가 하나 늘어 원인 추적 지점이 늘어난다.

### A2. `rhwp`를 Cargo git 의존성으로 링크

```toml
rhwp = { git = "https://github.com/edwardkim/rhwp", rev = "<pin>", default-features = false }
```

**장점**: 단일 바이너리 유지, 런타임 의존성 없음, in-process(빠름).

**단점**

- crates.io 배포 불가(git 의존성 금지). 우리는 배포 계획이 없어 치명적이진 않다.
- 의존성 24개를 상속한다. `image`(bmp/jpeg/png/tiff), `ttf-parser`,
  `wasm-bindgen`까지. CLI 플러그인이 `wasm-bindgen`을 끌어오는 건 어색하다.
  (다만 무거운 `resvg`/`skia-safe`는 `native-skia` 옵션이라 기본 꺼짐.)
- 바이너리가 현재보다 수 MB 커진다.
- RHWP 0.8.x가 빠르게 움직여 rev 고정 + 주기적 갱신 부담이 있다.

### A3. RHWP 파서를 우리 저장소에 벤더링

MIT라서 법적으로는 가능하다(저작권 표시 유지 필요). 하지만 250KB Rust를
포크로 안고 상류 개선을 따라가야 한다. **하지 않는다.**

### B. `unhwp` 직접 사용

폼 컨트롤이 사라진다(1-5). **하지 않는다.**

### C. HWP 5.0 파서 자체 구현

`cfb` + `flate2` + `byteorder`로 가능하고 `cfb`는 성숙하다(4,900만 다운로드).
하지만 DocInfo/BodyText/컨트롤/태그를 다 다루려면 RHWP가 250KB를 쓴 만큼의 일이다.
**지금은 하지 않는다.** 다만 A1의 장기 대안으로 남겨둔다(5절 R3).

### 결정

**A1을 1차로 구현한다.** 이유:

1. 새 포맷 지식이 필요 없다. 위험이 가장 낮다.
2. 우리 강점(폼 컨트롤)이 그대로 유지된다.
3. `.hwpx` 경로의 zero-install 성질을 훼손하지 않는다. HWP만 선택적 의존이다.
4. 프로토콜이 허용하는 패턴이고, officecli 자신도 프리빌트 배포라 생태계에 맞다.

A2는 "in-process가 꼭 필요해지면" 그때 검토한다. 결정 근거를 ADR로 남긴다.

## 3. 작업 계획

### H1. 확장자 등록과 디스커버리 (반나절)

- 매니페스트 `extensions`를 `[".hwpx", ".hwp"]`로 확장.
- 디스커버리는 `(kind, ext)`별 경로다(§3). 같은 바이너리를 두 경로에 둔다.
  §3이 "Symlinks are followed"라고 명시하므로 심볼릭 링크로 충분하다.

  ```
  ~/.officecli/plugins/dump-reader/hwpx/plugin   (실제 파일)
  ~/.officecli/plugins/dump-reader/hwp/plugin    → 위를 가리키는 심볼릭 링크
  ```
- `scripts/install.sh`에 `--with-hwp` 옵션 추가(기본 켜기, 링크 생성).
- 바이너리 이름 규약은 `officecli-<kind>-<ext>`이므로 PATH 경로 4순위를 쓰려면
  `officecli-dump-reader-hwp`도 필요하다. 링크로 처리한다.

**검증**: `officecli plugins list`에 `.hwpx, .hwp` 둘 다 나오는지.

### H2. 입력 포맷 판별 — **완료**

확장자만 믿지 않는다. 매직 바이트로 판별한다.

| 포맷 | 판별 |
|---|---|
| HWPX | ZIP 시그니처 `PK\x03\x04` + `mimetype` = `application/hwp+zip` |
| HWP 5.0 | CFB 시그니처 `D0 CF 11 E0 A1 B1 1A E1` |
| HWP 3.0 | 서명 문자열 `HWP Document File` |

`.hwp` 확장자인데 실제로는 HWPX인 파일이 흔하다(반대도 있다).
판별 결과로 경로를 고르면 확장자 오류에 강해진다.

**검증**: 확장자를 바꿔치기한 파일로 단위 테스트.

**결과**: `src/format.rs`. 단위 13개 + E2E 4개.

판별 규칙(실측으로 확정):

| 포맷 | 판별 |
|---|---|
| HWP 3.0 | 파일 **선두** 23바이트 `HWP Document File V3.00` |
| HWP 5.x | CFB(`D0 CF 11 E0 A1 B1 1A E1`) + `/FileHeader` 스트림이 `HWP Document File`로 시작 |
| HWPX | ZIP + `mimetype`이 `application/hwp+zip`, 또는 `Contents/section*.xml` 존재 |

구현 중 잡은 것들:

1. **CFB 시그니처만으로는 부족하다.** `.doc`/`.xls`가 같은 컨테이너다.
   `/FileHeader` 스트림을 실제로 열어 서명을 확인한다. 그래서 `cfb` 의존성을
   추가했다(MIT, rhwp도 같은 걸 쓴다). CFB 판별 코드와 함께 배포 크기는 소폭 늘었다.

2. **ZIP 시그니처는 계열로 봐야 한다.** 빈 아카이브는 `PK\x05\x06`(중앙
   디렉터리 끝)으로 시작해서 `PK\x03\x04`만 검사하면 놓친다. 분할 아카이브는
   `PK\x07\x08`이다. 단위 테스트에서 발견했다.

3. **HWP 3.0은 파일 선두에서만 인정한다.** CFB 파일의 `FileHeader` 스트림에도
   `HWP Document File` 문자열이 들어 있어서, 전체 검색을 하면 HWP 5.x를 3.0으로
   오판한다. CFB는 `D0 CF 11 E0`로 시작하므로 순서를 지키면 충돌하지 않는다.

4. **`FileHeader`에서 버전과 보호 상태를 함께 읽는다.** 암호·DRM 문서는 변환기도
   실패할 수 있으므로 미리 알린다. 레이아웃 근거는 rhwp `src/parser/header.rs`
   (버전 바이트 순서가 `revision, build, minor, major`인 점이 함정이다).

실측 결과:

```
hwp5   AI활용 아이디어 제안서_서식.hwp   version=5.1.0.1 compressed=true protection=none
hwpx   AI활용 아이디어 제안서_서식.hwpx
hwp5   swapped.hwpx                      version=5.1.0.1 compressed=true protection=none
error  garbage.hwp   [corrupt_input] unrecognized format (first bytes: 70 6C 61 69 6E 20 74 65)
```

H2 완료 당시 바이너리 HWP는 **exit 3**(`unsupported_feature`)으로 나가며
무엇을 해야 하는지 알렸다. H3 이후에도 변환기가 없으면 같은 계약을 유지한다:

```
$ officecli-dump-reader-hwpx dump 문서.hwp
[unsupported_feature] this is a binary HWP 5.x document (version 5.1.0.1), not
HWPX. Binary HWP support needs the optional RHWP converter. Install RHWP
v0.8.4+ on PATH or set OFFICECLI_HWPX_CONVERTER to its absolute path:
  rhwp export-hwpx <source> <target>.hwpx
(https://github.com/edwardkim/rhwp — MIT, prebuilt binaries available)
```

진단용 예제도 추가했다: `cargo run --release --example detect -- <파일>...`

**H3에서 바뀐 것**: `needs_conversion()`이 참일 때 변환기를 먼저 찾고, 없을 때만
기존 exit 3을 유지한다. 판별 결과(`SourceFormat`)를 그대로 넘기는 인터페이스를
재사용했다.

### H3. rhwp 브리지 — **완료**

```rust
/// `.hwp`를 HWPX로 변환해 임시 파일 경로를 돌려준다.
/// 변환기를 찾지 못하면 ExitCode::UnsupportedFeature(3).
fn convert_hwp_to_hwpx(src: &Path, media_dir: Option<&Path>)
    -> Result<Option<ConvertedHwpx>>
```

- 변환기 탐색 순서 (우리 프로젝트 관례를 프로토콜 §3에서 차용):
  1. `$OFFICECLI_HWPX_CONVERTER` (실행파일 절대경로)
  2. PATH의 `rhwp`
  3. `~/.local/rhwp/rhwp`
- 스크래치 위치: 호스트가 준 `--media-dir`을 우선 쓴다(§5.1이 "scratch directory
  the plugin may use for transient files"라고 정의한 그 용도다). 없으면 임시 디렉터리.
- 변환 산출물은 dump 종료 시 정리한다. 원본 옆에 파일을 만들지 않는다
  (§2.1은 소스를 읽기 전용으로 규정한다).
- 변환 실패는 exit 2(`corrupt_input`), 변환기 부재는 exit 3.
- 진단은 stderr/`--log-file`로. stdout은 JSONL 전용이다.
- `--verify`는 쓰지 않는다. 변환 시간이 늘고 우리 판단 대상이 아니다.

**검증**: 변환기 없는 환경에서 exit 3과 명확한 메시지. 손상 파일에서 exit 2.

**결과**: `src/converter.rs`. 탐색 우선순위는 절대경로 환경변수 → 절대 PATH
항목의 `rhwp` → 사용자 로컬 설치다. shell을 쓰지 않는다. RHWP v0.8.4가
비 UTF-8 argv를 받지 못하므로 원본을 UTF-8 고정명의 private `source.hwp`로
복사하고 `converted.hwpx`와 함께 staging 경로만 전달한다. 256MiB source-copy
예산, 120초 총 제한, 8KiB stderr tail, bounded drain, Unix process group, Windows Job Object,
regular-file 및 HWPX 재판별을 적용했다. 변환기 부재·안전한 containment 불가는
exit 3, 변환 실패·출력 누락·비 HWPX는 exit 2다. HWPX 직접 경로는 외부
변환기 없이 그대로 동작한다.

후속 보안 리뷰에서 scratch 권한과 프로세스 종료 경쟁도 닫았다. Unix는
`0700` directory/`0600` staged source와 `waitid(WNOWAIT)` 후 group kill을 사용한다.
Windows는 owner+SYSTEM protected DACL로 directory와 handle을 `NtCreateFile`에서
원자 생성하고 delete-share를 허용하지 않으며, Job active-process가 0이 된 뒤에만
산출물을 읽고 scratch를 정리한다. 임의의 Windows media root는 junction 재지정
위험 때문에 staging에 쓰지 않고 canonical user-temp root를 사용한다. RHWP
실행파일 경로 자체도 Unicode가 아니면 exit 3이다.

RHWP v0.8.4 `--verify`는 공식 표본 4종 중 3종에서 상류 IR 차이를 보고했다.
따라서 기존 결정대로 런타임 브리지에는 `--verify`를 넣지 않는다.

### H4. HWP↔HWPX 동등성 회귀 — **완료**

같은 문서의 두 포맷을 갖고 있으므로 **출력 대조**가 가능하다.
개인 코퍼스 기준선과 분리해 `scripts/verify-hwp-pairs.py`로 재현한다.

- `X.hwp`와 `X.hwpx`가 모두 있으면 두 JSONL의 요약을 비교한다.
- 비교 대상: 항목 수, 문단·표·셀·폼필드 개수, 텍스트 문자 다중집합.
- 기본 게이트는 요약과 OfficeCLI 왕복 구조 일치다. byte-for-byte JSONL 일치도
  별도로 보고하되 독립 편집된 쌍에는 강제하지 않는다.

**실측 결과**:

- 로컬 독립 동명 쌍 `AI활용 아이디어 제안서_서식`: 34개 JSONL byte-for-byte
  일치, unknown prop 0/0, OfficeCLI validate 0 error, 문단 27·표 1·셀 2 구조 일치.
- RHWP 공식 HWP5 3종·HWP3 1종과 v0.8.4가 만든 HWPX 쌍: 각각
  19/48/712/467개 JSONL exact, unknown prop 0, OfficeCLI 구조·스키마 일치.
- 모든 원본 hash·mtime 불변. 공식 4쌍은 같은 변환기로 만든 기준이라 브리지
  회귀에 강하고, 독립 저작 포맷 다양성 근거는 로컬 1쌍으로 제한된다.

### H5. 문서화 — **완료**

- README에 선택적 HWP 지원·RHWP 체크섬·실행 경계를 기록했다.
- `docs/01-protocol-contract.md`에 ADR-5(A1 선택)를 추가하고 ADR-2를 정정했다.
- `docs/02-handover.md`에 설명만 보고 판단해서 틀린 다섯 번째 사례를 기록했다.

### 순서

```
H2 판별  →  H3 브리지  →  H4 회귀  →  H5 문서  →  (원격 CI)  →  H1 디스커버리
```

H2를 먼저 하는 이유: 판별이 없으면 브리지가 HWPX 파일에도 변환을 걸어 낭비한다.

총 3일 예상. **새 파싱 코드는 없다.**

## 4. 검증 계획

각 단계 공통 절차는 `docs/03-work-plan.md` 4절과 같다. 추가되는 것:

| 항목 | 방법 |
|---|---|
| 포맷 판별 | 확장자 바꿔치기 파일 단위 테스트 |
| 변환기 부재 | `PATH`를 비우고 exit 3 확인 |
| 변환기 오류 | 손상 `.hwp`로 exit 2 확인 |
| 소스 불변 | 변환 전후 원본 mtime·해시 비교 |
| 임시파일 정리 | dump 후 스크래치 디렉터리가 비었는지 |
| HWP↔HWPX 동등성 | `verify-hwp-pairs.py`의 JSONL 요약·OfficeCLI 구조 비교 |
| 폼 컨트롤 보존 | HWP 경로에서도 체크박스 8 + 누름틀 3 나오는지 |
| 렌더 육안 확인 | HWP 경로 산출물 스크린샷 |

이미 확인한 것(계획 수립 중 실측):

```
h1 모집안내문   cb= 0 clk= 0 tbl= 1  items=193 unk=0  validate=OK
h2 참가신청서   cb= 8 clk= 4 tbl=10  items=207 unk=0  validate=OK
h3 참가신청서   cb= 8 clk= 4 tbl=10  items=207 unk=0  validate=OK
h4 AI 제안서    cb= 0 clk= 0 tbl= 1  items= 34 unk=0  validate=OK
```

## 5. 리스크

| # | 항목 | 대응 |
|---|---|---|
| R1 | rhwp 런타임 의존. `.hwp`가 조용히 실패하면 나쁘다 | exit 3 + 설치 안내 메시지. `--info`의 `supports`에 조건부임을 표시 검토 |
| R2 | rhwp 변환이 무손실이 아니다. 이중 변환(HWP→HWPX→docx)으로 손실이 겹칠 수 있다 | H4 동등성 회귀로 감시. rhwp `--verify`를 개발 중 진단으로만 사용 |
| R3 | rhwp가 0.8.x이고 빠르게 변한다. CLI 인터페이스가 바뀔 수 있다 | 버전을 확인하고(`rhwp --version`) 기대 범위를 기록. 깨지면 A2(라이브러리 링크) 또는 C(자체 파서)로 전환 |
| R4 | 암호 걸린 HWP | 보호 상태는 먼저 진단하지만 실제 변환 성공 여부는 표본이 없어 미검증. 실패 시 exit 2 |
| R5 | HWP 3.0 | 공식 HWP3 1종의 변환·467개 JSONL·OfficeCLI validate를 확인. 다양성은 여전히 부족 |
| R6 | 성능. 변환 단계가 늘어난다 | staging은 256MiB로 제한. HWPX 48MiB만 제한적 실측했고 대형 binary HWP와 느린 변환기 heartbeat-host 통합은 미실측 |
| R7 | 외부 바이너리 실행이라는 신뢰 경계 | 변환기 경로를 환경변수/PATH로만 받고, 다운로드 시 SHA256 대조를 스크립트에 넣는다 |

## 6. 정정한 기존 기록

`docs/01-protocol-contract.md` ADR-2는 이렇게 적혀 있다:

> ### ADR-2: `unhwp`를 파싱에 쓰지 않는다
> `unhwp`의 출력(Markdown/text/JSON)은 런 단위 서식·셀 병합·색상을 표현하지 못한다.

**전제가 틀렸다.** `unhwp`는 구조화된 모델을 노출한다(1-5).
결론(`unhwp`를 쓰지 않는다)은 유지되지만 **이유가 다르다**:
HWPX는 우리가 직접 파싱하는 것이 낫고(폼 컨트롤·정확한 길이 단위),
HWP는 `unhwp`에 폼 컨트롤이 없어서 못 쓴다.

H5에서 ADR-2를 정정하고 ADR-5(A1 채택)를 추가했다.

## 7. 하지 않을 것

- **HWP 쓰기.** dump-reader 계약 밖이다.
- **RHWP 벤더링/포크.** 유지보수 비용이 이득을 넘는다.
- **HWP 파서 자체 구현.** A1이 통하는 동안은 하지 않는다. R3에서 필요해지면 재검토.
- **`unhwp`를 폼 컨트롤 없이 채택.** 이 프로젝트의 존재 이유를 버리는 선택이다.
