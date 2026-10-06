//! TextMate scope palettes adapted to Fragile Notepad's neutral editor surfaces.
//! https://github.com/microsoft/vscode/tree/main/extensions/theme-defaults/themes
//! https://github.com/JetBrains/intellij-community/blob/master/platform/platform-resources/src/DefaultColorSchemesManager.xml
use super::{Theme, highlighting};

pub(super) fn theme(preset: Theme) -> highlighting::Theme {
    // Foreground, comment, keyword, control flow, string, number, type,
    // function, variable, constant, escape, invalid.
    let colors = match preset {
        Theme::VSCodeDark => [
            0xE8E9EB, 0x7F9F70, 0x6CB6F0, 0xD49BD3, 0xDE9E85, 0xB5CEA8, 0x5DD6BD, 0xE8CC8A,
            0x9CDCFE, 0x4FC1FF, 0xD7BA7D, 0xF48771,
        ],
        Theme::VSCodeLight => [
            0x202123, 0x537D42, 0x1756A9, 0x9636A2, 0xA31515, 0x098658, 0x167D8D, 0x806015,
            0x001080, 0x0070C1, 0xA65E00, 0xC72E32,
        ],
        Theme::JetBrainsDark => [
            0xE8E9EB, 0x92969E, 0xE69A5D, 0xE69A5D, 0x99BC80, 0x82ACD6, 0x78C3BF, 0xFFC66D,
            0xE8E9EB, 0xB493C7, 0xCCAA65, 0xF48771,
        ],
        Theme::JetBrainsLight => [
            0x202123, 0x737780, 0x000080, 0x000080, 0x008000, 0x0000FF, 0x267F99, 0x795E26,
            0x202123, 0x660E7A, 0xA65E00, 0xC72E32,
        ],
        Theme::SolarizedDark => [
            0xE8E9EB, 0x8A9A9E, 0x39ABEC, 0xBD80E7, 0x35C5AF, 0xE27DA7, 0xAACB44, 0xE8B642,
            0x8BC4DD, 0xF0A05A, 0xE8B642, 0xEF8585,
        ],
        Theme::SolarizedLight => [
            0x202123, 0x667D82, 0x0067AF, 0x8F35AB, 0x007D6C, 0xAF2E6B, 0x587B00, 0x946400,
            0x215E7D, 0xB65312, 0x946400, 0xC72E32,
        ],
        Theme::Base16Mocha => [
            0xE8E9EB, 0xA99C94, 0xD782C7, 0xD782C7, 0x9BC867, 0xF1A363, 0x58CBB7, 0xECC05D,
            0x91BFE5, 0xEB8077, 0xECC05D, 0xEF8585,
        ],
        Theme::MochaLight => [
            0x202123, 0x7B706B, 0x963B8D, 0x963B8D, 0x47751B, 0xAC5517, 0x087B68, 0x926200,
            0x235F91, 0xB53730, 0x926200, 0xC72E32,
        ],
        Theme::Base16Ocean => [
            0xE8E9EB, 0x8C9BAA, 0xBB8CF0, 0xBB8CF0, 0x9ACD70, 0xFAA76F, 0x49D0BF, 0x60AFF2,
            0x9CCDF3, 0xEC7C95, 0xEBC45C, 0xEF8585,
        ],
        Theme::OceanLight => [
            0x202123, 0x6B7A88, 0x7B3CB2, 0x7B3CB2, 0x407A18, 0xAC5119, 0x007D70, 0x086FB8,
            0x235F91, 0xB82E50, 0x956A00, 0xC72E32,
        ],
        Theme::Base16Eighties => [
            0xE8E9EB, 0x999999, 0xCE83F2, 0xCE83F2, 0xA0D24E, 0xFFAA55, 0x3CD1DB, 0x59B0F5,
            0x9ACCF0, 0xF47777, 0xF2C64F, 0xEF8585,
        ],
        Theme::EightiesLight => [
            0x202123, 0x747474, 0x903CB9, 0x903CB9, 0x507B0A, 0xB15A0A, 0x007D88, 0x0873B8,
            0x235F91, 0xBF3232, 0x956A00, 0xC72E32,
        ],
        Theme::InspiredGitHub => [
            0x202123, 0x6E7781, 0xCF222E, 0xCF222E, 0x064B83, 0x0550AE, 0x007580, 0x8034CF,
            0x953800, 0x0550AE, 0x9A6700, 0xC72E32,
        ],
        Theme::GitHubDark => [
            0xE8E9EB, 0x8B949E, 0xFF746C, 0xFF746C, 0x76BEF5, 0x58B5FF, 0x43D6BD, 0xC58AFF,
            0xFFA657, 0x79C0FF, 0xE3B341, 0xFF7B72,
        ],
    };
    let color = |rgb: u32| highlighting::Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 255,
    };
    let mut result = highlighting::Theme::default();
    result.name = Some(preset.to_string());
    result.settings.foreground = Some(color(colors[0]));
    result.settings.background = Some(color(if preset.is_dark() { 0x1A1B1D } else { 0xFFFFFF }));
    for (scope, index) in [
        ("comment", 1),
        ("keyword, storage", 2),
        ("keyword.control", 3),
        ("string", 4),
        ("constant.numeric", 5),
        (
            "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.namespace, support.type, support.class",
            6,
        ),
        (
            "entity.name.function, entity.name.macro, support.function, support.macro, variable.function",
            7,
        ),
        ("variable, entity.name.variable", 8),
        ("constant.language", 2),
        (
            "constant.other, variable.other.constant, variable.other.enummember",
            9,
        ),
        ("entity.name.tag", 2),
        ("entity.other.attribute-name", 8),
        ("constant.character.escape", 10),
        ("keyword.operator", 0),
        ("punctuation", 0),
        ("invalid", 11),
        ("markup.heading", 2),
        ("markup.inline.raw, markup.raw", 4),
        ("markup.underline.link", 6),
        ("markup.inserted", 5),
        ("markup.deleted", 11),
    ] {
        result.scopes.push(highlighting::ThemeItem {
            scope: scope.parse().expect("valid built-in scope selector"),
            style: highlighting::StyleModifier {
                foreground: Some(color(colors[index])),
                ..Default::default()
            },
        });
    }
    result
}
