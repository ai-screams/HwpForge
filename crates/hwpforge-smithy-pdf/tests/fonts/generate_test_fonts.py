#!/usr/bin/env python3
"""HwpForge 테스트 폰트 생성기 (fontTools).

CI 커버리지용 자체 제작 미니 TTF 2종을 만든다 — 외부 폰트 재배포가 아니라
이 스크립트가 처음부터 그리는 산출물이므로 라이선스 제약이 없다.

의도적 메트릭 (셰이퍼/정렬 단언이 이 값에 고정된다):
- upem = 1000
- space  advance = 300  (0.3em — 한컴 0.5em 오버라이드가 폰트값을 이기는지 검증용)
- Latin/숫자/문장부호 advance = 600 (0.6em)
- 한글 음절 advance = 1000 (1.0em 전각)

파일 2종:
- HwpForgeTest-Regular.ttf  (subfamily "Regular" — family 이름 등록 대상)
- HwpForgeTest-Bold.ttf     (subfamily "Bold" — resolver 의 regular-only
  family 필터를 CI 에서 잠그기 위한 충돌 상대. 파일명이 Regular 보다
  사전순으로 앞서므로 필터가 없으면 family 를 선점한다.)

재생성: `python3 generate_test_fonts.py` (fontTools 필요).
"""

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPM = 1000
LATIN = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.,:-"
HANGUL = "가나다라마바사아자차카타파하"


def rect_glyph(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x1, y0))
    pen.lineTo((x1, y1))
    pen.lineTo((x0, y1))
    pen.closePath()
    return pen.glyph()


def empty_glyph():
    return TTGlyphPen(None).glyph()


def build(style_name: str, out_path: Path) -> None:
    order = [".notdef", "space"] + [f"uni{ord(c):04X}" for c in LATIN + HANGUL]
    cmap = {0x20: "space"}
    glyf = {".notdef": rect_glyph(50, 0, 550, 700), "space": empty_glyph()}
    metrics = {".notdef": (600, 50), "space": (300, 0)}
    for c in LATIN:
        name = f"uni{ord(c):04X}"
        cmap[ord(c)] = name
        glyf[name] = rect_glyph(50, 0, 550, 700)
        metrics[name] = (600, 50)
    for c in HANGUL:
        name = f"uni{ord(c):04X}"
        cmap[ord(c)] = name
        glyf[name] = rect_glyph(50, 0, 950, 700)
        metrics[name] = (1000, 50)

    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyf)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable(
        {
            "familyName": "HwpForge Test",
            "styleName": style_name,
            "fullName": f"HwpForge Test {style_name}",
            "psName": f"HwpForgeTest-{style_name}",
            "version": "1.000",
        }
    )
    fb.setupOS2(
        sTypoAscender=800,
        sTypoDescender=-200,
        usWinAscent=800,
        usWinDescent=200,
        usWeightClass=700 if style_name == "Bold" else 400,
    )
    fb.setupPost()
    fb.save(str(out_path))
    print(f"wrote {out_path} ({out_path.stat().st_size} bytes)")


if __name__ == "__main__":
    here = Path(__file__).parent
    build("Regular", here / "HwpForgeTest-Regular.ttf")
    build("Bold", here / "HwpForgeTest-Bold.ttf")
