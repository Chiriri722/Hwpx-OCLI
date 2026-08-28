#!/usr/bin/env python3
"""전 기능 HWPX 픽스처 생성기 (검증용, Rust 코드와 독립).

Rust 테스트 픽스처 빌더가 틀렸을 때 같이 틀리지 않도록 별도 구현으로 만든다.
실제 officecli 바이너리와의 왕복 검증에 쓴다.

    python3 scripts/make_fixture.py out.hwpx
"""
import sys
import zipfile

NS = (
    ' xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"'
    ' xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"'
    ' xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"'
    ' xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core"'
)

# 1x1 투명 PNG
TINY_PNG = bytes([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
    0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
])

HEADER = (
    '<?xml version="1.0" encoding="UTF-8"?>'
    f'<hh:head{NS} version="1.4" secCnt="1"><hh:refList>'
    '<hh:charProperties itemCnt="3">'
    # id 0: 본문 10pt 검정
    '<hh:charPr id="0" height="1000" textColor="#000000">'
    '<hh:fontRef hangul="함초롬바탕" latin="Times New Roman"/></hh:charPr>'
    # id 1: 제목 18pt 굵게 남색
    '<hh:charPr id="1" height="1800" textColor="#1F4E79">'
    '<hh:fontRef hangul="함초롬돋움" latin="Arial"/><hh:bold/></hh:charPr>'
    # id 2: 강조 10pt 빨강 기울임+밑줄+취소선
    '<hh:charPr id="2" height="1000" textColor="#C00000">'
    '<hh:italic/><hh:underline type="BOTTOM"/><hh:strikeout type="SINGLE"/></hh:charPr>'
    '</hh:charProperties>'
    '<hh:paraProperties itemCnt="4">'
    '<hh:paraPr id="0"/>'
    # id 1: 가운데, 문단 뒤 여백 800 hwpunit(=160twip=8pt)
    '<hh:paraPr id="1"><hh:align horizontal="CENTER"/>'
    '<hh:margin><hh:next value="800"/></hh:margin></hh:paraPr>'
    # id 2: 양쪽, 첫줄 들여쓰기 1000, 줄간격 160%
    '<hh:paraPr id="2"><hh:align horizontal="JUSTIFY"/>'
    '<hh:margin><hh:intent value="1000"/><hh:left value="2000"/></hh:margin>'
    '<hh:lineSpacing type="PERCENT" value="160"/></hh:paraPr>'
    # id 3: 내어쓰기. HWP는 음수 intent로 표현한다. 실제 문서 형식대로
    # hc: 네임스페이스와 hp:switch 이중 구조를 쓴다.
    '<hh:paraPr id="3">'
    '<hp:switch>'
    '<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">'
    '<hh:margin><hc:intent value="-8570" unit="HWPUNIT"/>'
    '<hc:left value="8570" unit="HWPUNIT"/></hh:margin></hp:case>'
    '<hp:default>'
    '<hh:margin><hc:intent value="-8570" unit="HWPUNIT"/>'
    '<hc:left value="8570" unit="HWPUNIT"/></hh:margin></hp:default>'
    '</hp:switch>'
    '</hh:paraPr>'
    '</hh:paraProperties></hh:refList></hh:head>'
)


def run(cp, inner):
    return f'<hp:run charPrIDRef="{cp}">{inner}</hp:run>'


def wrap(pp, inner):
    return (
        f'<hp:p id="0" paraPrIDRef="{pp}" styleIDRef="0">{inner}'
        '<hp:linesegarray><hp:lineSeg textpos="0" vertpos="0"/></hp:linesegarray>'
        '</hp:p>'
    )


def para(cp, pp, text):
    return wrap(pp, run(cp, f'<hp:t>{text}</hp:t>'))


def multi_run_para(pp, parts):
    return wrap(pp, ''.join(run(cp, f'<hp:t>{t}</hp:t>') for cp, t in parts))


def linebreak_para(cp, pp, a, b):
    return wrap(pp, run(cp, f'<hp:t>{a}</hp:t>')
                + run(cp, '<hp:lineBreak/>')
                + run(cp, f'<hp:t>{b}</hp:t>'))


def cell(r, c, text, cspan=1, rspan=1, width=2600, fill=None):
    brush = ''
    if fill:
        brush = ('<hp:cellBrush>'
                 f'<hc:fillBrush faceColor="{fill}" hatchColor="#000000"/>'
                 '</hp:cellBrush>')
    return (
        '<hp:tc borderFillIDRef="1">'
        f'<hp:subList>{para("0", "0", text)}</hp:subList>'
        f'<hp:cellAddr colAddr="{c}" rowAddr="{r}"/>'
        f'<hp:cellSpan colSpan="{cspan}" rowSpan="{rspan}"/>'
        f'<hp:cellSz width="{width}" height="1000"/>'
        f'{brush}</hp:tc>'
    )


def table(rows, cols, cells_by_row):
    trs = ''.join(f'<hp:tr>{"".join(cs)}</hp:tr>' for cs in cells_by_row)
    return wrap('0', run('0',
        f'<hp:tbl id="1" rowCnt="{rows}" colCnt="{cols}" borderFillIDRef="1">'
        f'<hp:sz width="7800" height="3000"/>{trs}</hp:tbl>'))


def checkbox(name, checked):
    """hp:checkBtn 폼 컨트롤. 실측 문서의 구조를 그대로 따른다."""
    value = 'CHECKED' if checked else 'UNCHECKED'
    return (
        f'<hp:checkBtn caption="" value="{value}" radioGroupName="" triState="0"'
        f' backStyle="1" name="{name}" foreColor="#000000" backColor="#FFFFFF"'
        ' groupName="" tabStop="1" editable="1" tabOrder="2" enabled="1"'
        ' borderTypeIDRef="0" drawFrame="1" printable="1" command="">'
        '<hp:formCharPr charPrIDRef="0" followContext="0" autoSz="0" wordWrap="0"/>'
        '<hp:sz width="1168" widthRelTo="ABSOLUTE" height="1433"'
        ' heightRelTo="ABSOLUTE" protect="0"/>'
        '<hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1"/>'
        '</hp:checkBtn>'
    )


def checkbox_para(cp, text, name, checked):
    return wrap('0', run(cp, f'<hp:t>{text}</hp:t>' + checkbox(name, checked)))


def nested_checkbox_table(cp):
    """중첩표 안의 체크박스. 실측에서 이 경로가 유실됐다."""
    inner_cells = (
        '<hp:tc borderFillIDRef="1">'
        f'<hp:subList>{checkbox_para(cp, "청소년부 ", "CBNest1", False)}</hp:subList>'
        '<hp:cellAddr colAddr="0" rowAddr="0"/>'
        '<hp:cellSpan colSpan="1" rowSpan="1"/>'
        '<hp:cellSz width="2600" height="1000"/></hp:tc>'
        '<hp:tc borderFillIDRef="1">'
        f'<hp:subList>{checkbox_para(cp, "대학·일반부 ", "CBNest2", True)}</hp:subList>'
        '<hp:cellAddr colAddr="1" rowAddr="0"/>'
        '<hp:cellSpan colSpan="1" rowSpan="1"/>'
        '<hp:cellSz width="2600" height="1000"/></hp:tc>'
    )
    inner = wrap('0', run(cp,
        '<hp:tbl id="9" rowCnt="1" colCnt="2" borderFillIDRef="1">'
        '<hp:sz width="5200" height="1000"/>'
        f'<hp:tr>{inner_cells}</hp:tr></hp:tbl>'))
    outer_cell = (
        '<hp:tc borderFillIDRef="1">'
        f'<hp:subList>{inner}</hp:subList>'
        '<hp:cellAddr colAddr="0" rowAddr="0"/>'
        '<hp:cellSpan colSpan="1" rowSpan="1"/>'
        '<hp:cellSz width="5200" height="1000"/></hp:tc>'
    )
    return wrap('0', run(cp,
        '<hp:tbl id="8" rowCnt="1" colCnt="1" borderFillIDRef="1">'
        '<hp:sz width="5200" height="1000"/>'
        f'<hp:tr>{outer_cell}</hp:tr></hp:tbl>'))


def picture(bin_id, w, h, alt):
    return wrap('0', run('0',
        '<hp:pic reverse="0" id="2" zOrder="1" textWrap="SQUARE">'
        f'<hp:sz width="{w}" height="{h}"/>'
        f'<hp:img binaryItemIDRef="{bin_id}" bright="0" contrast="0" '
        f'effect="REAL_PIC" alt="{alt}"/>'
        '<hp:imgRect><hc:pt0 x="0" y="0"/></hp:imgRect></hp:pic>'))


def build(path):
    body = ''.join([
        # 1. 제목 — 단일 서식, 가운데, 18pt 굵게
        para('1', '1', '분기 보고서'),
        # 2. 혼합 서식 — 문단 + 런 3개로 쪼개져야 함
        multi_run_para('2', [
            ('0', '매출은 '),
            ('2', '전년 대비 12% 증가'),
            ('0', '했습니다.'),
        ]),
        # 3. 문단 내 줄바꿈 — 문단이 쪼개지면 안 됨
        linebreak_para('0', '0', '첫 번째 줄', '같은 문단 둘째 줄'),
        # 4. 표 — 가로 병합 + 배경색
        table(3, 3, [
            [cell(0, 0, '구분', fill='#EDEDED'),
             cell(0, 1, '1분기', fill='#EDEDED'),
             cell(0, 2, '2분기', fill='#EDEDED')],
            [cell(1, 0, '매출'), cell(1, 1, '1,200'), cell(1, 2, '1,344')],
            [cell(2, 0, '비고: 전 항목 확정', cspan=3)],
        ]),
        # 5. 세로 병합 + 행 중간 가로 병합
        #    격자:  [ 세로병합 ][  가로병합(2열)  ]
        #           [   〃     ][ 좌 ][ 우 ]
        table(2, 3, [
            [cell(0, 0, '세로병합', rspan=2), cell(0, 1, '가로병합', cspan=2)],
            [cell(1, 1, '좌'), cell(1, 2, '우')],
        ]),
        # 6. 폼 컨트롤 체크박스 (문단 직속)
        checkbox_para('0', '동의합니다 ', 'CBTop', True),
        # 7. 중첩표 안의 체크박스 — 실측에서 유실됐던 경로
        nested_checkbox_table('0'),
        # 8. 이미지
        picture('chart1', 7200, 3600, '매출 추이'),
        # 9. 내어쓰기 (음수 hc:intent) — hangingIndent로 나가야 한다
        para('0', '3', '가. 내어쓰기 문단입니다. 둘째 줄부터 들여쓰기가 유지됩니다.'),
        # 10. 한컴 사용자 정의 영역 문자 — 치환하지 않고 보고만 한다
        para('0', '0', '사용자정의 \uF0854특수\uF0855 문자'),
        # 11. 엔티티
        para('0', '0', '각주 &amp; 참고 &lt;자료&gt; &quot;인용&quot;'),
        # 12. 탭
        wrap('0', run('0', '<hp:t>왼쪽</hp:t>')
             + run('0', '<hp:tab/>')
             + run('0', '<hp:t>오른쪽</hp:t>')),
    ])

    section = f'<?xml version="1.0" encoding="UTF-8"?><hs:sec{NS}>{body}</hs:sec>'
    hpf = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>'
        '<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>'
        '<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>'
        '<opf:item id="chart1" href="BinData/chart1.png" media-type="image/png"/>'
        '</opf:manifest><opf:spine>'
        '<opf:itemref idref="header" linear="yes"/>'
        '<opf:itemref idref="section0" linear="yes"/>'
        '</opf:spine></opf:package>'
    )

    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr('mimetype', 'application/hwp+zip', zipfile.ZIP_STORED)
        z.writestr('Contents/content.hpf', hpf)
        z.writestr('Contents/header.xml', HEADER)
        z.writestr('Contents/section0.xml', section)
        z.writestr('BinData/chart1.png', TINY_PNG)
    return path


if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else 'fixture.hwpx'
    print(build(out))
