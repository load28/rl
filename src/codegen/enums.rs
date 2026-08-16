//! rl `enum` emission: a discriminated-union `type` plus a constructor
//! `const` object, both under the enum's original name.

use crate::ast::EnumDecl;
use crate::scanner::*;

pub(super) fn emit_enum(decl: &EnumDecl) -> String {
    let EnumDecl {
        name,
        exported,
        generics,
        cases,
    } = decl;
    let exp = if *exported { "export " } else { "" };

    let type_arms: Vec<String> = cases
        .iter()
        .map(|c| match &c.fields {
            Some(fields) if !fields.is_empty() => {
                let list = fields
                    .iter()
                    .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ kind: \"{}\"; {} }}", c.tag, list)
            }
            _ => format!("{{ kind: \"{}\" }}", c.tag),
        })
        .collect();
    let type_decl = format!(
        "{}type {}{} =\n  | {};",
        exp,
        name,
        generics,
        type_arms.join("\n  | ")
    );

    let type_args = if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generic_param_names(generics).join(", "))
    };
    let ctors: Vec<String> = cases
        .iter()
        .map(|c| match &c.fields {
            None => {
                // unit case: a singleton value, kept narrow via `as const`
                format!("  {}: {{ kind: \"{}\" }} as const,", c.tag, c.tag)
            }
            Some(fields) => {
                let params = fields
                    .iter()
                    .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut obj = vec![format!("kind: \"{}\"", c.tag)];
                obj.extend(fields.iter().map(|f| f.name.clone()));
                format!(
                    "  {}: {}({}): {}{} => ({{ {} }}),",
                    c.tag,
                    generics,
                    params,
                    name,
                    type_args,
                    obj.join(", ")
                )
            }
        })
        .collect();
    let const_decl = format!("{}const {} = {{\n{}\n}};", exp, name, ctors.join("\n"));

    format!("{}\n{}", type_decl, const_decl)
}

/// `<T extends string, U = number>` → ["T", "U"]
fn generic_param_names(generics: &str) -> Vec<String> {
    let inner = &generics[1..generics.len() - 1];
    let src = inner.as_bytes();
    let end = src.len();
    let mut names = Vec::new();
    let mut i = 0usize;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end || !is_ident_start(src[i]) {
            break;
        }
        let j = ident_end(src, i, end);
        let word = &inner[i..j];
        if word == "const" || word == "in" || word == "out" {
            // modifier — the actual name follows
            i = j;
            continue;
        }
        names.push(word.to_string());
        i = scan_type_end(src, j, end); // skip constraint/default up to the next comma
        if at(src, i, end) == Some(b',') {
            i += 1;
        }
    }
    names
}
