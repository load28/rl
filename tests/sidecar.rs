//! Editor sidecar tests: `.rl.d.ts` + `.rl.d.ts.map`.
//!
//! The map is what makes "go to definition" from a `.ts` importer land in
//! the original `.rl`, so these tests decode it and check the positions
//! rather than comparing opaque VLQ strings.

use rlc::build_sidecar;

const SOURCE: &str = r#"import { pad } from "./format.ts";

/** 알림 한 건. */
export enum Notice {
  Info(text: string),
  Warn(text: string),
}

export function render(notice: Notice): string {
  return match (notice) {
    Info(text) => pad(text, 4),
    Warn(text) => pad(text, 4),
  };
}
"#;

const DECLARATIONS: &str = r#"/** 알림 한 건. */
export type Notice = {
    kind: "Info";
    text: string;
} | {
    kind: "Warn";
    text: string;
};
export declare const Notice: {
    Info: (text: string) => Notice;
    Warn: (text: string) => Notice;
};
export declare function render(notice: Notice): string;
"#;

/// One decoded mapping segment: where a generated position points to.
#[derive(Debug, PartialEq, Eq)]
struct Segment {
    generated_line: usize,
    generated_column: usize,
    source_line: usize,
    source_column: usize,
}

fn decode(mappings: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let (mut source_line, mut source_column) = (0i64, 0i64);
    for (generated_line, line) in mappings.split(';').enumerate() {
        let mut generated_column = 0i64;
        if line.is_empty() {
            continue;
        }
        for field in line.split(',') {
            let values = decode_vlq(field);
            assert_eq!(values.len(), 4, "segment should carry four fields");
            generated_column += values[0];
            source_line += values[2];
            source_column += values[3];
            out.push(Segment {
                generated_line,
                generated_column: generated_column as usize,
                source_line: source_line as usize,
                source_column: source_column as usize,
            });
        }
    }
    out
}

fn decode_vlq(field: &str) -> Vec<i64> {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut value, mut shift) = (0i64, 0);
    for byte in field.bytes() {
        let digit = DIGITS
            .iter()
            .position(|d| *d == byte)
            .expect("base64 digit") as i64;
        value += (digit & 31) << shift;
        if digit & 32 != 0 {
            shift += 5;
            continue;
        }
        let magnitude = value >> 1;
        out.push(if value & 1 == 1 {
            -magnitude
        } else {
            magnitude
        });
        value = 0;
        shift = 0;
    }
    out
}

fn field(map: &str, key: &str) -> String {
    let start = map.find(key).expect("key present") + key.len();
    let rest = &map[start..];
    let end = rest.find(['"']).map_or(rest.len(), |i| i);
    rest[..end].to_string()
}

#[test]
fn declarations_open_with_a_generated_banner() {
    // The file sits next to the source (TypeScript resolves `./x.rl` to a
    // sibling `x.rl.d.ts` and nowhere else), so it says what it is.
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "notice.rl");
    assert!(
        sidecar
            .declarations
            .starts_with("// @generated from notice.rl by rlc --sidecar"),
        "{}",
        sidecar.declarations
    );
}

#[test]
fn declarations_carry_a_source_mapping_url() {
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "notice.rl");
    assert!(
        sidecar
            .declarations
            .trim_end()
            .ends_with("//# sourceMappingURL=notice.rl.d.ts.map"),
        "{}",
        sidecar.declarations
    );
}

#[test]
fn an_existing_source_mapping_url_is_replaced_not_duplicated() {
    let input = format!("{DECLARATIONS}//# sourceMappingURL=notice.d.ts.map\n");
    let sidecar = build_sidecar(SOURCE, &input, "notice.rl");
    assert_eq!(sidecar.declarations.matches("sourceMappingURL").count(), 1);
    assert!(!sidecar.declarations.contains("notice.d.ts.map\n//#"));
}

#[test]
fn map_names_the_rl_file_as_its_source() {
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "notice.rl");
    assert!(
        sidecar.map.contains("\"sources\":[\"notice.rl\"]"),
        "{}",
        sidecar.map
    );
    assert!(
        sidecar.map.contains("\"file\":\"notice.rl.d.ts\""),
        "{}",
        sidecar.map
    );
}

#[test]
fn a_relative_source_path_records_the_distance_but_not_in_the_file_names() {
    // With `-o` the declarations live in their own tree, which TypeScript
    // merges back via `rootDirs`. Only `sources` carries the distance —
    // the map and the banner still name the file itself.
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "../src/notice.rl");
    assert!(
        sidecar.map.contains("\"sources\":[\"../src/notice.rl\"]"),
        "{}",
        sidecar.map
    );
    assert!(
        sidecar.map.contains("\"file\":\"notice.rl.d.ts\""),
        "{}",
        sidecar.map
    );
    assert!(
        sidecar
            .declarations
            .starts_with("// @generated from notice.rl by rlc --sidecar"),
        "{}",
        sidecar.declarations
    );
    assert!(
        sidecar
            .declarations
            .contains("//# sourceMappingURL=notice.rl.d.ts.map"),
        "{}",
        sidecar.declarations
    );
}

#[test]
fn every_declaration_maps_to_its_position_in_the_rl_source() {
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "notice.rl");
    let segments = decode(&field(&sidecar.map, "\"mappings\":\""));

    // `export enum Notice` is on source line 4 (zero-based 3), name at
    // column 12. Both the type alias and the constructor const map there.
    let notice: Vec<&Segment> = segments.iter().filter(|s| s.source_line == 3).collect();
    assert!(!notice.is_empty(), "{segments:?}");
    assert!(notice.iter().all(|s| s.source_column == 12), "{notice:?}");
    // Generated lines are shifted by one: line 0 is the @generated banner.
    assert_eq!(
        notice.iter().map(|s| s.generated_line).collect::<Vec<_>>(),
        vec![2, 2, 9, 9],
        "type alias and constructor const both point at the enum"
    );

    // `export function render` is on source line 9 (zero-based 8), name at
    // column 16.
    let render: Vec<&Segment> = segments.iter().filter(|s| s.source_line == 8).collect();
    assert!(!render.is_empty(), "{segments:?}");
    assert!(render.iter().all(|s| s.source_column == 16), "{render:?}");
}

#[test]
fn each_declaration_gets_a_segment_at_its_name_column() {
    // Go-to-definition asks about the column where the name starts; a
    // segment only at column 0 leaves the editor stranded in the .d.ts.
    let sidecar = build_sidecar(SOURCE, DECLARATIONS, "notice.rl");
    let segments = decode(&field(&sidecar.map, "\"mappings\":\""));

    // "render" starts at column 24 of `export declare function render(...)`.
    assert!(
        segments
            .iter()
            .any(|s| s.source_line == 8 && s.generated_column == 24),
        "{segments:?}"
    );
    assert!(segments.iter().any(|s| s.generated_column == 0));
}

#[test]
fn declarations_with_no_source_match_are_skipped_without_panicking() {
    let sidecar = build_sidecar(SOURCE, "export declare const ghost: number;\n", "notice.rl");
    let segments = decode(&field(&sidecar.map, "\"mappings\":\""));
    assert!(segments.is_empty(), "{segments:?}");
}
