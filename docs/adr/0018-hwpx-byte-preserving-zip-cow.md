# ADR-0018: HWPX 저장은 ZIP header를 재생성하지 않는다

상태: 채택 · 2026-09-30 · 기준 커밋: `5431668a` 이후 작업 트리 ·
[ADR-0013](0013-hancom-package-preserving-editor-policy.md)의 ZIP metadata 결정을 보완한다.

## 배경

2026-09-29 검토에서 Python 기본 `zipfile.writestr`로 만든 패키지는 텍스트 저장이
`unchanged part "mimetype" did not preserve its payload and ZIP metadata`로 거부됐다.
원인은 writer였다. 기존 COW는 `zip` crate의 `raw_copy_file`/`start_file`로 모든
entry의 header를 다시 만들었고, crate가 Unix mode에 regular-file 비트를 강제로
더했다(`0o600 << 16` → `0o100600 << 16`). G3가 이 변화를 정확히 거부했다.

같은 재생성은 G3가 비교하지 않던 필드도 바꿨다. 로컬 사용자 HWPX 2개(내용은 읽지
않고 ZIP header만 확인)와 rhwp 공개 표본(Hancom 10~13 생성) 12개를 보면 native 패키지는
`version made by` 0x0B17, 외부 속성 `0x81800020`(DOS archive 비트 포함), deflate
entry의 general-purpose flag `0x4`를 쓴다. 09-29 빌드로 공개 표본 8개를 편집·저장하면
모두 성공으로 보고됐지만 91개 entry의 273개 header 필드가 바뀌었다
(`version needed` 20→10, `0x20`·`0x4` 제거). 오류 없이 metadata가 손실된 것이다.

## 결정

1. 편집 저장은 classic 단일 디스크 ZIP의 raw layout을 직접 읽고 쓴다
   (`owpml::zip_layout`). 바뀌지 않은 entry는 local header·payload·data descriptor와
   central record를 바이트 그대로 복사한다. 교체 entry는 CRC-32와 두 크기 필드만
   바꾸고 나머지 header 바이트는 원본을 유지한다. 위치(offset)와 end record의 central
   directory 위치만 재배치로 바뀐다. no-op 복사는 원본과 바이트가 같다.
2. G3와 TOCTOU snapshot은 decoded metadata 외에 정확한 local header, descriptor,
   offset을 가린 central record와 end record를 비교한다. 교체 entry는 payload 필드만
   가리고 비교한다. 따라서 DOS 속성·flag·internal attribute 변화도 거부한다.
3. 다음 layout은 editable open에서 `unsupported_feature`로 거부한다. 편집한 뒤 저장
   단계에서 비로소 실패하지 않게 하기 위해서다. 대상: ZIP64, 다중 디스크,
   entry 사이/central directory 앞뒤/end record 뒤의 여분 바이트, 물리 순서와
   central directory 순서 불일치, 암호화, stored/deflate 외 압축, local/central 이름·
   크기 불일치, 인식하지 못한 data descriptor. 교체 대상 part에 ZIP extra field가
   있으면 여전히 거부한다(바뀌지 않은 part의 extra field는 이제 보존한다).
4. G0~G2 판정과 허용 범위는 바꾸지 않았다. 검사를 완화한 것이 아니라 writer가
   원본을 재현하게 한 것이다.

## 근거와 검증

- 수정 전 커밋에서 새 COW 테스트 4개가 실패하고 수정 후 통과했다: 바뀌지 않은 part의
  extra field 보존, 생산자 metadata 보존, data descriptor 보존, G3의 DOS 비트·flag·
  internal attribute 변화 검출.
- 재현 harness: Python writer 세 방식이 수정 전에는 저장 거부, 수정 후에는 저장·재열기
  성공. native 모양 metadata의 header 차이는 수정 전 3종에서 수정 후 없음으로 바뀌었다.
- 공개 표본 12개 중 편집 가능한 8개가 저장 후 header 필드 차이 0이다. 2개는 기존
  strict G0~G2 검사가 editable open에서 거부한다(manifest의 비이식 경로
  `D:\다운로드\`, 압축된 `mimetype`). 나머지 2개는 plain text 대상이 없다.
- 이 결정은 표본이 관측한 layout에 대한 보존 계약이다. 모든 ZIP writer 호환을 뜻하지
  않고, 한글 프로그램의 열기·렌더 결과를 보증하지 않는다.
