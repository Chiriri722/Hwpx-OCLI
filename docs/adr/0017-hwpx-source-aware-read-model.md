# ADR-0017: HWPX 원본 위치를 보존하는 직접 읽기

상태: 채택 · 2026-09-29 · 기준 커밋: `5431668a`

## 결정

직접 format-handler의 기존 문단/텍스트 인덱스에 표, 셀, 각주/미주, 필드
마커와 원본 위치를 추가한다. DOCX projection 모델은 셀 주소를 보정하고 XML
위치를 버리므로 원본 편집 주소로 역변환하지 않는다. 기존 namespace-aware
스캔의 XML 이벤트를 재사용하며 외부 구현이나 새 파서 의존성은 추가하지 않는다.

기존 `/document/section[n]/paragraph[n]/text[n]` 주소는 유지한다. 구조 노드는
구역별 종류/출현 순서 주소를 가지며 `parent_path`, `paragraph_paths`,
`text_paths`로 관계를 표현한다. 문단을 구조 아래로 이동하거나 복제하지 않는다.
셀 좌표/병합 값은 원문에 있는 값만 공개하고 누락된 좌표는 추측하지 않는다.
필드 이름은 식별자가 아니다. 시작/끝 ID가 각각 유일하고 순서·subList 영역이 맞을 때만
필드 내용 범위를 연결한다. 그 외에는 미해결 상태를 공개한다.

2026-09-30 보완: 그림(`hp:pic`)도 같은 방식의 읽기 전용 구조 노드로 공개한다. 그림
자신의 직접 자식만 사용한다. core 이름공간 `hc:img`의 `binaryItemIDRef`, 원문
HWPUNIT 값인 `hp:orgSz`/`hp:curSz`/`hp:sz`, 하나뿐일 때의 `hp:shapeComment`가 대상이다.
binary 참조는 OPF 이름공간의 `opf:manifest/opf:item` 중 id가 정확히 하나이고
`href`가 실제 package part일 때만 해결한다. DOCX 변환 reader의 파일명 stem
폴백은 쓰지 않는다. 외부 연결(`isEmbeded="0"`), 누락, 중복 id, 없는 part, 읽을 수 없는
manifest는 상태 값으로 공개한다. 이미지 바이트 추출·교체·크기 변경은 제공하지 않는다.

원본 참조는 ZIP part, 해당 part의 SHA-256, UTF-8 XML 바이트의 반개구간이다.
`set`의 선택적 `expected_revision`은 이 해시를 검사한다. 수정 후 재조회해야
하며 서로 다른 part의 해시는 문서 전체 revision으로 취급하지 않는다.

편집 가능성은 실제 세션 권한과 기존 plain-text writer의 범위에서 계산한다.
읽기 전용의 수정 후보는 현재 권한과 별도로 공개하며 strict 패키지 검증을 대신하지 않는다.
구조 컨테이너의 직접 수정, self-closing 텍스트 확장, 필드 이름 기반 자동 치환,
자동 재배치, 렌더 보증은 제공하지 않는다. 빈 paired `hp:t`와 분할 런은 기존
명시적 text 경로로 편집한다. `outline`/`issues`는 구현 전까지 명시적으로
`unsupported_feature`를 반환하며 `validate`의 패키지 검사와 구분한다.

## 근거와 검증

사용자가 제공한 `Pro.txt`와 kordoc 공개 README의 구조/원본 연결 개념을
검토했으며 외부 소스는 복사·번역하지 않는다. XML 이름/속성은 저장소의
OWPML parser와 기존 합성 fixtures를 근거로 한다. hwp-mcp의 native 확인
방식은 별도 실제 한글 환경이 있을 때만 적용하며 실행하지 않은 렌더 검증을
완료로 표시하지 않는다. 엄격한 인적 분리를 갖춘 법적 clean-room 인증은 아니다.

수용 기준: namespace 구분, 병합/빈 셀/중첩 구조, 중복 필드 이름과 모호한 ID,
소스 범위/해시, 읽기 전용 capability, 오래된 revision 거부, 기존 set/save/reopen
및 G0~G3 회귀. XML 깊이/항목 제한은 정확한 허용값과 첫 거부값을 검사한다.

그림 조회는 rhwp 공개 표본(커밋 `496333b2`, Hancom 10~13 생성)의 구조를 근거로 한다.
`test-image.hwpx` 5개, 표 안의 `tb-img-03.hwpx` 1개, `paper_anchor_infront_pic.hwpx` 1개가
해결되고, `issue1891_external_bindata_link.hwpx`는 외부 연결 4개와 내장 2개로 구분된다.
텍스트 변경 후 `hp:linesegarray` 처리는 계속 보류한다. RHWP v0.8.4(한글이 아닌 독립
렌더러)에서는 표 셀의 오래된 캐시 때문에 긴 텍스트가 한 줄로 셀 밖에 넘쳤다. 캐시를
지우면 줄바꿈은 됐지만 병합 셀의 행 높이는 늘지 않았다. 구역 전체 캐시 삭제는 편집하지
않은 원본의 쪽수도 6쪽에서 5쪽으로 바꿨다. 따라서 삭제 여부와 범위는 한글 결과로만
정한다. 한글 자동화 검증 도구는 준비했지만 실행 승인 전에는 결론으로 쓰지 않는다.
