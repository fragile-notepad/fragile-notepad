"""Regional body-font catalog shared by build preparation and the renderer.

Lists contain actual font family names, not filenames or PostScript names.
Regional Noto/Source Han collections take precedence as a complete design;
individual lists apply only when no complete collection is installed. UI
variants follow body families because their punctuation can be proportional.

WenQuanYi is only a Simplified Chinese fallback: its broad Unicode coverage
does not make its Han forms suitable for Japanese, Korean or Traditional
Chinese. Likewise, IPA/Takao and Nanum/Un fonts stay in their own regions.
Serif and older bitmap-oriented families are last-resort regional body faces.

Name references:
https://learn.microsoft.com/typography/font-list
https://support.apple.com/en-sg/120414
https://moji.or.jp/ipafont/releasenote00303/
https://github.com/google/fonts/tree/main/ofl/nanumgothic
https://sources.debian.org/src/fonts-arphic-uming/0.2.20080216.2-11/NEWS
https://packages.debian.org/sid/fonts/fonts-unfonts-core
https://launchpad.net/takao-fonts
https://sourceforge.net/p/wqy/news/2009/05/wenquanyi-micro-hei-02beta-is-released/
"""

from __future__ import annotations

LANGUAGES = ("sc", "tc", "jp", "kr")
TARGET_OSES = ("windows", "linux", "macos")

COHERENT_COLLECTIONS = (
    (
        ("Noto Sans Mono CJK SC",),
        ("Noto Sans Mono CJK TC",),
        ("Noto Sans Mono CJK JP",),
        ("Noto Sans Mono CJK KR",),
    ),
    (
        ("Noto Sans CJK SC",),
        ("Noto Sans CJK TC",),
        ("Noto Sans CJK JP",),
        ("Noto Sans CJK KR",),
    ),
    (
        ("Noto Sans SC",),
        ("Noto Sans TC",),
        ("Noto Sans JP",),
        ("Noto Sans KR",),
    ),
    (
        ("Source Han Sans SC", "Source Han Sans CN"),
        ("Source Han Sans TC", "Source Han Sans TW"),
        ("Source Han Sans JP", "Source Han Sans"),
        ("Source Han Sans KR", "Source Han Sans K"),
    ),
)

_COMMON = tuple(
    tuple(dict.fromkeys(name for collection in COHERENT_COLLECTIONS for name in collection[index]))
    for index in range(len(LANGUAGES))
)

PREFERRED_FAMILIES = {
    "windows": (
        ("Microsoft YaHei", *_COMMON[0], "DengXian", "SimHei", "SimSun", "Microsoft YaHei UI"),
        (
            "Microsoft JhengHei", *_COMMON[1], "MingLiU", "PMingLiU",
            "MingLiU_HKSCS", "Microsoft JhengHei UI",
        ),
        (
            "Yu Gothic", *_COMMON[2], "Meiryo", "MS Gothic",
            "MS PGothic", "MS Mincho", "Yu Gothic UI",
        ),
        ("Malgun Gothic", *_COMMON[3], "Gulim", "Dotum", "GulimChe", "DotumChe", "Batang"),
    ),
    "macos": (
        ("PingFang SC", *_COMMON[0], "Hiragino Sans GB", "Heiti SC", "Songti SC"),
        ("PingFang TC", *_COMMON[1], "Heiti TC", "PingFang HK", "Songti TC"),
        (
            "Hiragino Sans", *_COMMON[2], "Hiragino Kaku Gothic ProN",
            "Hiragino Kaku Gothic Pro", "Osaka",
        ),
        ("Apple SD Gothic Neo", *_COMMON[3], "AppleGothic", "AppleMyungjo"),
    ),
    "linux": (
        (*_COMMON[0], "WenQuanYi Micro Hei", "WenQuanYi Zen Hei", "AR PL UMing CN"),
        (*_COMMON[1], "AR PL UMing TW", "AR PL UMing TW MBE"),
        (*_COMMON[2], "IPAexGothic", "IPAGothic", "TakaoExGothic", "TakaoGothic", "VL Gothic"),
        (
            *_COMMON[3], "NanumGothic", "Nanum Gothic", "NanumBarunGothic",
            "Nanum Barun Gothic", "UnDotum", "UnBatang",
        ),
    ),
}


def preferred_families(target_os: str) -> tuple[tuple[str, ...], ...]:
    """Return SC, TC, JP and KR candidates for a supported Rust target OS."""
    try:
        return PREFERRED_FAMILIES[target_os]
    except KeyError as error:
        raise ValueError(f"Unsupported font target OS: {target_os!r}") from error


def coherent_collections() -> tuple[tuple[tuple[str, ...], ...], ...]:
    """Return complete regional designs, each in SC, TC, JP and KR order."""
    return COHERENT_COLLECTIONS
