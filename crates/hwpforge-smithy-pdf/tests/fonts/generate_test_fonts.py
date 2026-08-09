#!/usr/bin/env python3
"""HwpForge 테스트 폰트 생성기 (fontTools).

CI 커버리지용 자체 제작 미니 TTF 를 만든다 — 외부 폰트 재배포가 아니라
이 스크립트가 처음부터 그리는 산출물이므로 라이선스 제약이 없다.
SOURCE_DATE_EPOCH=0 으로 고정해 재생성이 바이트 결정적이다.

의도적 메트릭 (셰이퍼/정렬 단언이 이 값에 고정된다):
- upem = 1000
- space  advance = 300  (0.3em — 한컴 0.5em 오버라이드가 폰트값을 이기는지 검증용)
- Latin/숫자/문장부호 advance = 600 (0.6em) — W4 Bold 는 **700 (0.7em)**
  으로 실제 폭이 다르다 (outline 도 폭이 달라 bbox 검증이 유효).
- 한글 음절 advance = 1000 (1.0em 전각)

파일 그룹:
- W2 레거시 쌍 (파라미터 불변 유지 — 기존 게이트가 이 형태에 고정):
  - HwpForgeTest-Regular.ttf  (subfamily "Regular" — family 이름 등록 대상)
  - HwpForgeTest-Bold.ttf     (subfamily "Bold" + **regular 플래그** — W4a
    분류기의 "신호 모순 = ambiguous" 레거시 케이스를 겸한다.)
- W4a 분류기 fixture:
  - HwpForgeW4-{Regular,Bold}.ttf — 정상 Bold 축 (nameID 16/17 typographic
    이름 · fsSelection BOLD · macStyle · weight 700 · 실제 폭 상이).
    nameID 1 은 의도적으로 다른 이름("HwpForgeW4 Legacy") — 16 우선 검증.
  - HwpForgeConflict-Bold.ttf — subfamily "Bold" + regular 플래그 + weight
    400 (모순 → ambiguous 전용 케이스).
  - HwpForgeRank-{R400,R500}.ttf — 같은 (family, Regular) 후보 2개, weight
    ranking (400 최근접) 검증.
  - HwpForgeRankTie-{R350,R450}.ttf — 목표 400 에서 동거리 → ambiguous.
- W4d fsType fixture (라이선스 게이트 진리표 — §5 H4):
  - HwpForgeFsV0Restricted.ttf  (OS/2 v0, fsType=0x0002 — ENGDOS 실물 형태)
  - HwpForgeFsV2NoSubset.ttf    (v2, fsType=0x0100 — bit8 No subsetting)
  - HwpForgeFsV2BitmapOnly.ttf  (v2, fsType=0x0200 — bit9 Bitmap only)
  - HwpForgeFsV2Multi.ttf       (v2, fsType=0x0104 — P&P + bit8 복수비트)
  - HwpForgeFsV3Malformed.ttf   (v3, fsType=0x0001 — 예약 비트 세트)
  - HwpForgeFsNoOs2.ttf         (OS/2 테이블 결측)

재생성: `python3 generate_test_fonts.py` (fontTools 필요).
"""

import os

os.environ["SOURCE_DATE_EPOCH"] = "0"  # head.created/modified 고정 (결정적 재생성)

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPM = 1000
LATIN = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.,:-"
HANGUL = "가나다라마바사아자차카타파하"

FS_SELECTION_ITALIC = 0x0001
FS_SELECTION_BOLD = 0x0020
MAC_STYLE_BOLD = 0x0001


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


def build(
    out_path: Path,
    *,
    family: str,
    style_name: str,
    full_name: str,
    ps_name: str,
    typo_family: str | None = None,
    typo_style: str | None = None,
    latin_adv: int = 600,
    weight: int = 400,
    bold_flags: bool = False,
    os2_version: int | None = None,
    fs_type: int | None = None,
    drop_os2: bool = False,
) -> None:
    """단일 스타일 TTF 하나를 만든다. Latin outline 폭은 advance 에 연동."""
    latin_x1 = latin_adv - 50  # advance 가 다르면 outline 폭(잉크)도 다르다
    order = [".notdef", "space"] + [f"uni{ord(c):04X}" for c in LATIN + HANGUL]
    cmap = {0x20: "space"}
    glyf = {".notdef": rect_glyph(50, 0, latin_x1, 700), "space": empty_glyph()}
    metrics = {".notdef": (latin_adv, 50), "space": (300, 0)}
    for c in LATIN:
        name = f"uni{ord(c):04X}"
        cmap[ord(c)] = name
        glyf[name] = rect_glyph(50, 0, latin_x1, 700)
        metrics[name] = (latin_adv, 50)
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
    names = {
        "familyName": family,
        "styleName": style_name,
        "fullName": full_name,
        "psName": ps_name,
        "version": "1.000",
    }
    if typo_family is not None:
        names["typographicFamily"] = typo_family
    if typo_style is not None:
        names["typographicSubfamily"] = typo_style
    fb.setupNameTable(names)
    os2 = {
        "sTypoAscender": 800,
        "sTypoDescender": -200,
        "usWinAscent": 800,
        "usWinDescent": 200,
        "usWeightClass": weight,
        # fontTools 기본값은 0x0004(Preview&Print) — 자체 제작 폰트는 무제약이
        # 진실이므로 installable(0) 로 명시 (W4d 라이선스 게이트 노이즈 방지).
        "fsType": 0x0000,
    }
    if bold_flags:
        os2["fsSelection"] = FS_SELECTION_BOLD
    if os2_version is not None:
        os2["version"] = os2_version
    if fs_type is not None:
        os2["fsType"] = fs_type
    fb.setupOS2(**os2)
    fb.setupPost()
    if bold_flags:
        fb.font["head"].macStyle = MAC_STYLE_BOLD
    if drop_os2:
        del fb.font["OS/2"]
    fb.save(str(out_path))
    print(f"wrote {out_path} ({out_path.stat().st_size} bytes)")


if __name__ == "__main__":
    here = Path(__file__).parent

    # W2 레거시 쌍 — 파라미터 불변 (Bold 는 의도적으로 플래그 미설정 모순체).
    for style in ("Regular", "Bold"):
        build(
            here / f"HwpForgeTest-{style}.ttf",
            family="HwpForge Test",
            style_name=style,
            full_name=f"HwpForge Test {style}",
            ps_name=f"HwpForgeTest-{style}",
            weight=700 if style == "Bold" else 400,
        )

    # W4a 정상 Bold 축 쌍 — nameID 16/17 + 진짜 플래그 + 실제 폭 차이.
    build(
        here / "HwpForgeW4-Regular.ttf",
        family="HwpForgeW4 Legacy",
        style_name="Regular",
        full_name="HwpForge W4 Regular",
        ps_name="HwpForgeW4-Regular",
        typo_family="HwpForge W4",
        typo_style="Regular",
    )
    build(
        here / "HwpForgeW4-Bold.ttf",
        family="HwpForgeW4 Legacy",
        style_name="Bold",
        full_name="HwpForge W4 Bold",
        ps_name="HwpForgeW4-Bold",
        typo_family="HwpForge W4",
        typo_style="Bold",
        latin_adv=700,
        weight=700,
        bold_flags=True,
    )

    # 모순 전용 (subfamily "Bold" + regular 플래그 + weight 400).
    build(
        here / "HwpForgeConflict-Bold.ttf",
        family="HwpForge Conflict",
        style_name="Bold",
        full_name="HwpForge Conflict Bold",
        ps_name="HwpForgeConflict-Bold",
    )

    # weight ranking (Regular 목표 400): 400 이 500 을 이겨야 한다.
    build(
        here / "HwpForgeRank-R400.ttf",
        family="HwpForge Rank",
        style_name="Regular",
        full_name="HwpForge Rank R400",
        ps_name="HwpForgeRank-R400",
    )
    build(
        here / "HwpForgeRank-R500.ttf",
        family="HwpForge Rank",
        style_name="Regular",
        full_name="HwpForge Rank R500",
        ps_name="HwpForgeRank-R500",
        weight=500,
    )

    # weight 동률 (350/450 — 목표 400 에서 동거리) → ambiguous.
    build(
        here / "HwpForgeRankTie-R350.ttf",
        family="HwpForge RankTie",
        style_name="Regular",
        full_name="HwpForge RankTie R350",
        ps_name="HwpForgeRankTie-R350",
        weight=350,
    )
    build(
        here / "HwpForgeRankTie-R450.ttf",
        family="HwpForge RankTie",
        style_name="Regular",
        full_name="HwpForge RankTie R450",
        ps_name="HwpForgeRankTie-R450",
        weight=450,
    )

    # W4d fsType 진리표 fixture (§5 H4) — 분류기(W4a)는 이들을 정상 해석한다.
    for suffix, os2_version, fs_type, drop in (
        ("FsV0Restricted", 0, 0x0002, False),
        ("FsV2NoSubset", 2, 0x0100, False),
        ("FsV2BitmapOnly", 2, 0x0200, False),
        ("FsV2Multi", 2, 0x0104, False),
        ("FsV3Malformed", 3, 0x0001, False),
        ("FsNoOs2", None, None, True),
    ):
        build(
            here / f"HwpForge{suffix}.ttf",
            family=f"HwpForge {suffix}",
            style_name="Regular",
            full_name=f"HwpForge {suffix} Regular",
            ps_name=f"HwpForge{suffix}-Regular",
            os2_version=os2_version,
            fs_type=fs_type,
            drop_os2=drop,
        )
