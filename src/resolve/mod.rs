//! Name resolution — rl names bound to declarations, by identity.
//!
//! This is the resolve phase of the compiler core
//! (`docs/design/compiler-core.md` §5): declaration collection builds the
//! world one file sees — its own enums, its imported ones, the built-ins —
//! as definitions with stable IDs, and reference resolution binds every
//! name a pattern writes to one of them. After this phase, "is this tag
//! that case?" is an ID comparison, never a string comparison: two enums
//! may both declare a `Circle`, and the two `Circle`s are different
//! definitions.
//!
//! rl-specific shape: rustc's resolver knows the scrutinee's type before
//! it resolves a pattern's path; rlc does not (types are TypeScript's).
//! So resolution here starts with **subject identification** — which enum
//! a site's tags collectively name — using exactly the rules
//! [`crate::analysis`] established (all-tags candidate first, then the
//! unique best-overlap holder; silence on a tie or no overlap, because tag
//! patterns legitimately match hand-written unions rl has no declaration
//! for). The two implementations are pinned to each other by equivalence
//! tests until Phase 3 moves the analysis onto this one.
//!
//! Like [`crate::hir`], this surface is exported for language tooling but
//! is not a stable API.

use std::collections::HashMap;

use crate::hir::{
    self, Arena, DefId, FieldBinding, HirFile, NodeId, Pat, PatternId, PatternSiteId, SiteKind,
    Span,
};

/// An imported declaration as the resolver receives it: the name it is
/// known by in the importing file's scope (aliases applied), where it came
/// from, and its variants — with payload fields when the collector had
/// them ([`crate::EnumSymbol`]), tags only otherwise ([`crate::ExternEnum`]).
#[derive(Debug, Clone)]
pub struct ExternDecl {
    /// The name in the importing file's scope.
    pub name: String,
    /// The import specifier, when recorded.
    pub from: Option<String>,
    /// The variants.
    pub variants: Vec<ExternVariant>,
}

/// One variant of an [`ExternDecl`]: the tag, and — when the collector had
/// them — the payload fields.
pub type ExternVariant = (String, Option<Vec<ExternField>>);

/// One payload field of an [`ExternVariant`]: `(name, optional, type text)`.
pub type ExternField = (String, bool, String);

impl From<&crate::ExternEnum> for ExternDecl {
    fn from(e: &crate::ExternEnum) -> ExternDecl {
        ExternDecl {
            name: e.name.clone(),
            from: e.from.clone(),
            variants: e.tags.iter().map(|t| (t.clone(), None)).collect(),
        }
    }
}

impl From<&crate::EnumSymbol> for ExternDecl {
    fn from(e: &crate::EnumSymbol) -> ExternDecl {
        ExternDecl {
            name: e.name.clone(),
            from: None,
            variants: e
                .cases
                .iter()
                .map(|c| {
                    (
                        c.tag.clone(),
                        c.fields.as_ref().map(|fields| {
                            fields
                                .iter()
                                .map(|f| (f.name.clone(), f.optional, f.ty.clone()))
                                .collect()
                        }),
                    )
                })
                .collect(),
        }
    }
}

/// Resolves one lowered file against its imported declarations. Records
/// definition name spans into the file's source map (local declarations
/// only — an import or a built-in has no position in this file).
pub fn resolve_file(hir: &mut HirFile, externs: &[ExternDecl]) -> Resolution {
    let mut resolver = Resolver {
        resolution: Resolution::default(),
    };
    resolver.collect(hir, externs);
    resolver.resolve_sites(hir);
    resolver.resolution
}

/// The resolve phase's answer for one file.
#[derive(Debug, Default)]
pub struct Resolution {
    /// Every definition the file can see, local, imported and built-in.
    pub defs: Arena<DefId, Definition>,
    /// The type namespace after shadowing: name → definition.
    pub type_ns: HashMap<String, DefId>,
    /// The value namespace after shadowing: name → definition (an rl enum
    /// contributes its constructor object here).
    pub value_ns: HashMap<String, DefId>,
    /// Each pattern site's identified subject, one per scrutinee position
    /// (`None` where the tags name no known enum).
    pub sites: HashMap<PatternSiteId, SiteResolution>,
    /// Every resolved name use, keyed by the name's HIR node: a pattern's
    /// tag, a pattern's payload field. This is what definition/hover/rename
    /// consume — the node is the use, the [`Res`] is what it means.
    pub uses: HashMap<NodeId, Res>,
    /// Every name that failed to resolve *and* that resolution can name a
    /// replacement for, in source order — the diagnostic's raw material.
    pub unresolved: Vec<UnresolvedUse>,
}

impl Resolution {
    /// The definition a name resolves to in `ns`, after shadowing.
    pub fn lookup(&self, ns: Namespace, name: &str) -> Option<DefId> {
        match ns {
            Namespace::Type => self.type_ns.get(name).copied(),
            Namespace::Value => self.value_ns.get(name).copied(),
        }
    }

    /// The enum definition behind `def` — itself for a type-namespace
    /// enum, the owner for a constructor-object value.
    pub fn enum_of(&self, def: DefId) -> Option<(DefId, &EnumDef)> {
        match &self.defs[def].kind {
            DefKind::Enum(data) => Some((def, data)),
            DefKind::EnumValue { enum_def } => match &self.defs[*enum_def].kind {
                DefKind::Enum(data) => Some((*enum_def, data)),
                _ => None,
            },
        }
    }

    /// A variant by reference.
    pub fn variant(&self, at: VariantRef) -> Option<&VariantDecl> {
        let (_, data) = self.enum_of(at.enum_def)?;
        data.variants.get(at.index as usize)
    }

    /// A field by reference.
    pub fn field(&self, at: FieldRef) -> Option<&FieldDecl> {
        self.variant(at.variant)?
            .fields
            .as_ref()?
            .get(at.index as usize)
    }
}

/// The namespaces rl names live in. Pattern binding locals get their own
/// space when the resolver takes over local scopes (Phase 5's flow work
/// feeds it); enum variants and payload fields are addressed through their
/// owner, not by bare name, so they need no top-level namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    /// Type positions: `Shape` in `x: Shape`.
    Type,
    /// Value positions: `Shape` in `Shape.Circle(2)`.
    Value,
}

/// One definition.
#[derive(Debug)]
pub struct Definition {
    /// The name this definition is known by **in this file's scope** —
    /// import aliases applied, `ns.Name` for a namespace import.
    pub name: String,
    /// What it is.
    pub kind: DefKind,
}

/// What a definition is. An rl enum mints two: the type ([`DefKind::Enum`])
/// and the constructor object ([`DefKind::EnumValue`]), one per namespace —
/// variants and fields hang off the enum and are addressed by
/// [`VariantRef`]/[`FieldRef`], which keeps their identity tied to their
/// owner.
#[derive(Debug)]
pub enum DefKind {
    /// An enum type, with its variants.
    Enum(EnumDef),
    /// An enum's constructor object in the value namespace.
    EnumValue {
        /// The type-namespace definition this constructor object belongs to.
        enum_def: DefId,
    },
}

/// An enum definition's data.
#[derive(Debug)]
pub struct EnumDef {
    /// Where the declaration lives.
    pub origin: DeclOrigin,
    /// The variants, in declaration order — a [`VariantRef`]'s index
    /// indexes this.
    pub variants: Vec<VariantDecl>,
}

/// Where a definition came from — what a diagnostic names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclOrigin {
    /// Declared in this file; the node is the declared name.
    Local(NodeId),
    /// Imported (the specifier, when the collector recorded it).
    Imported {
        /// The import specifier as written, e.g. `./token.rl`.
        from: Option<String>,
    },
    /// A built-in (`Option`, `Result`) — declaration identity without a
    /// declaration site in user code.
    Builtin,
}

/// One variant of an [`EnumDef`].
#[derive(Debug)]
pub struct VariantDecl {
    /// The tag.
    pub name: String,
    /// The declaring tag's node, for a local declaration.
    pub node: Option<NodeId>,
    /// The lowered syntax this declaration came from, for a local one —
    /// the bridge between the declaration world and the file's HIR.
    pub hir: Option<hir::VariantId>,
    /// The payload fields; `None` for a unit case, and for imported
    /// declarations that carried tags only.
    pub fields: Option<Vec<FieldDecl>>,
}

/// One payload field of a [`VariantDecl`].
#[derive(Debug)]
pub struct FieldDecl {
    /// The field name.
    pub name: String,
    /// The declaring name's node, for a local declaration.
    pub node: Option<NodeId>,
    /// The lowered syntax, for a local declaration.
    pub hir: Option<hir::FieldId>,
    /// Whether the field is optional.
    pub optional: bool,
    /// The verbatim declared type text.
    pub ty_text: String,
}

/// A variant, addressed through its owner — the identity form of "the
/// `Circle` of `Shape`", never confusable with another enum's `Circle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariantRef {
    /// The owning enum's definition.
    pub enum_def: DefId,
    /// Index into the owner's variant list.
    pub index: u32,
}

/// A field, addressed through its variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldRef {
    /// The owning variant.
    pub variant: VariantRef,
    /// Index into the variant's field list.
    pub index: u32,
}

/// What a name use resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Res {
    /// A top-level definition.
    Def(DefId),
    /// An enum variant.
    Variant(VariantRef),
    /// A payload field.
    Field(FieldRef),
    /// Nothing this file can see. Deliberately not an error by itself —
    /// tag patterns match hand-written unions too; the reportable subset
    /// is [`Resolution::unresolved`].
    Unresolved,
    /// More than one candidate and no rule to pick one (reserved: today's
    /// identification rules answer a unique candidate or stay silent).
    Ambiguous(Vec<DefId>),
}

/// One pattern site's identified subjects.
#[derive(Debug)]
pub struct SiteResolution {
    /// One entry per scrutinee position: the enum that position's tags
    /// identify, or `None` when they name no known enum.
    pub subjects: Vec<Option<DefId>>,
}

/// A name use that failed to resolve, with the replacement that licenses
/// reporting it (`docs/design/rust-parity-analysis.md` GAP-1: no
/// suggestion, no error — an unknown name may be a hand-written union's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedUse {
    /// The site the name was written in — the recovery boundary a
    /// reporter suppresses that site's coverage question by.
    pub site: PatternSiteId,
    /// The name's node (span = the name as written).
    pub node: NodeId,
    /// The name as written.
    pub name: String,
    /// Case tag or payload field.
    pub kind: UseKind,
    /// The enum the name was resolved against.
    pub against: DefId,
    /// For a field, the case whose payload it was looked up in.
    pub tag: Option<String>,
    /// The declared name it looks like a misspelling of — same-domain
    /// only: a case suggestion comes from `against`'s own variants, a
    /// field suggestion from the named case's own fields.
    pub suggestion: String,
}

/// What kind of name an [`UnresolvedUse`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    /// A case tag.
    Case,
    /// A payload field name.
    Field,
}

struct Resolver {
    resolution: Resolution,
}

impl Resolver {
    /// Declaration collection: local enums (a later declaration of the
    /// same name replaces an earlier one, as in the analysis' registry),
    /// then imported ones (shadowed by locals), then the built-ins
    /// (shadowed by both) — `Option` and `Result` enter as definitions
    /// like any other, not as string special cases.
    fn collect(&mut self, hir: &mut HirFile, externs: &[ExternDecl]) {
        let locals: Vec<(String, EnumDef, Option<Span>)> = hir
            .items
            .iter()
            .filter_map(|item| {
                let hir::Item::Enum(decl) = item else {
                    return None;
                };
                let variants = decl
                    .variants
                    .iter()
                    .map(|&variant_id| {
                        let variant = &hir.variants[variant_id];
                        VariantDecl {
                            name: variant.name.clone(),
                            node: Some(variant.node),
                            hir: Some(variant_id),
                            fields: variant.fields.as_ref().map(|fields| {
                                fields
                                    .iter()
                                    .map(|&field_id| {
                                        let field = &hir.fields[field_id];
                                        FieldDecl {
                                            name: field.name.clone(),
                                            node: Some(field.node),
                                            hir: Some(field_id),
                                            optional: field.optional,
                                            ty_text: field.ty_text.clone(),
                                        }
                                    })
                                    .collect()
                            }),
                        }
                    })
                    .collect();
                Some((
                    decl.name.clone(),
                    EnumDef {
                        origin: DeclOrigin::Local(decl.node),
                        variants,
                    },
                    hir.source_map.node_span(decl.node),
                ))
            })
            .collect();
        for (name, data, span) in locals {
            self.declare_enum(name, data, span, hir);
        }
        for extern_decl in externs {
            if self.resolution.type_ns.contains_key(&extern_decl.name) {
                continue; // shadowed by a local declaration
            }
            self.declare_enum(
                extern_decl.name.clone(),
                EnumDef {
                    origin: DeclOrigin::Imported {
                        from: extern_decl.from.clone(),
                    },
                    variants: extern_decl
                        .variants
                        .iter()
                        .map(|(tag, fields)| VariantDecl {
                            name: tag.clone(),
                            node: None,
                            hir: None,
                            fields: fields.as_ref().map(|fields| {
                                fields
                                    .iter()
                                    .map(|(name, optional, ty)| FieldDecl {
                                        name: name.clone(),
                                        node: None,
                                        hir: None,
                                        optional: *optional,
                                        ty_text: ty.clone(),
                                    })
                                    .collect()
                            }),
                        })
                        .collect(),
                },
                None,
                hir,
            );
        }
        for (name, variants) in builtin_enums() {
            if self.resolution.type_ns.contains_key(&name) {
                continue; // shadowed by a local or imported declaration
            }
            self.declare_enum(
                name,
                EnumDef {
                    origin: DeclOrigin::Builtin,
                    variants,
                },
                None,
                hir,
            );
        }
    }

    fn declare_enum(&mut self, name: String, data: EnumDef, span: Option<Span>, hir: &mut HirFile) {
        // A later local declaration of the same name replaces the earlier
        // one in both namespaces (last one wins, as in the analysis).
        let enum_def = if let Some(&existing) = self.resolution.type_ns.get(&name) {
            self.resolution.defs[existing].kind = DefKind::Enum(data);
            existing
        } else {
            let enum_def = self.resolution.defs.alloc(Definition {
                name: name.clone(),
                kind: DefKind::Enum(data),
            });
            let value_def = self.resolution.defs.alloc(Definition {
                name: name.clone(),
                kind: DefKind::EnumValue { enum_def },
            });
            self.resolution.type_ns.insert(name.clone(), enum_def);
            self.resolution.value_ns.insert(name, value_def);
            enum_def
        };
        if let Some(span) = span {
            hir.source_map.record_def(enum_def, span);
        }
    }

    /// Reference resolution over every pattern site, in source order.
    fn resolve_sites(&mut self, hir: &HirFile) {
        let site_ids: Vec<PatternSiteId> = hir.sites.iter().map(|(id, _)| id).collect();
        for site_id in site_ids {
            self.resolve_site(hir, site_id);
        }
        self.resolution.unresolved.sort_by_key(|u| {
            hir.source_map
                .node_span(u.node)
                .map_or(usize::MAX, |s| s.start)
        });
    }

    fn resolve_site(&mut self, hir: &HirFile, site_id: PatternSiteId) {
        let site = &hir.sites[site_id];
        let single_pattern = matches!(site.kind, SiteKind::IfLet | SiteKind::LetElse);
        let positions = site.subjects.len();
        let mut subjects: Vec<Option<DefId>> = Vec::with_capacity(positions);
        for position in 0..positions {
            // The evidence: every tag the site's arms use at this position
            // (all alternatives of an or-pattern, guarded arms included).
            let mut tags: Vec<&str> = Vec::new();
            for arm in &site.arms {
                collect_position_tags(hir, arm.pattern, position, positions, &mut tags);
            }
            let subject = self.identify(&tags);
            // Resolve this position's constructor uses against the
            // identified enum — or, for a single-pattern site whose one
            // tag identified nothing, under the strict one-edit licence.
            match subject {
                Some(enum_def) => {
                    for arm in &site.arms {
                        self.resolve_position(
                            hir,
                            site_id,
                            arm.pattern,
                            position,
                            positions,
                            enum_def,
                        );
                    }
                }
                None if single_pattern => {
                    for arm in &site.arms {
                        self.report_near_miss(hir, site_id, arm.pattern);
                    }
                }
                None => {}
            }
            subjects.push(subject);
        }
        self.resolution
            .sites
            .insert(site_id, SiteResolution { subjects });
    }

    /// The subject identification rule, shared with the analysis (see the
    /// module docs): an enum containing **every** tag, in shadowing order;
    /// otherwise the unique enum containing the most of them (at least
    /// one); a tie or no overlap identifies nothing.
    fn identify(&self, tags: &[&str]) -> Option<DefId> {
        if tags.is_empty() {
            return None;
        }
        let enums: Vec<(DefId, &EnumDef)> = self
            .resolution
            .defs
            .iter()
            .filter_map(|(id, def)| match &def.kind {
                DefKind::Enum(data) => Some((id, data)),
                DefKind::EnumValue { .. } => None,
            })
            .filter(|(id, _)| {
                // Only names visible after shadowing count: a replaced
                // local or a shadowed built-in is not a candidate.
                self.resolution.type_ns.get(&self.resolution.defs[*id].name) == Some(id)
            })
            .collect();
        let has = |data: &EnumDef, tag: &str| data.variants.iter().any(|v| v.name == tag);
        if let Some((id, _)) = enums
            .iter()
            .find(|(_, data)| tags.iter().all(|t| has(data, t)))
        {
            return Some(*id);
        }
        let mut best: Option<(DefId, usize)> = None;
        let mut tied = false;
        for (id, data) in &enums {
            let hits = tags.iter().filter(|t| has(data, t)).count();
            if hits == 0 {
                continue;
            }
            match best {
                Some((_, most)) if hits < most => {}
                Some((_, most)) if hits == most => tied = true,
                _ => {
                    best = Some((*id, hits));
                    tied = false;
                }
            }
        }
        if tied { None } else { best.map(|(id, _)| id) }
    }

    /// Resolves the constructor uses of `pattern` at `position` (of
    /// `positions`) against `enum_def`, recording a [`Res`] per name node
    /// and an [`UnresolvedUse`] where a near-miss licenses one.
    fn resolve_position(
        &mut self,
        hir: &HirFile,
        site: PatternSiteId,
        pattern: PatternId,
        position: usize,
        positions: usize,
        enum_def: DefId,
    ) {
        match &hir.patterns[pattern] {
            Pat::Wildcard | Pat::Literal(_) => {}
            Pat::Or(alts) => {
                for &alt in alts {
                    self.resolve_position(hir, site, alt, position, positions, enum_def);
                }
            }
            Pat::Tuple(elems) => {
                if positions > 1 {
                    if let Some(&elem) = elems.get(position) {
                        self.resolve_position(hir, site, elem, 0, 1, enum_def);
                    }
                } else {
                    for &elem in elems {
                        self.resolve_position(hir, site, elem, 0, 1, enum_def);
                    }
                }
            }
            Pat::Constructor { path, fields } => {
                self.resolve_constructor(hir, site, path, fields.as_deref(), enum_def);
            }
        }
    }

    fn resolve_constructor(
        &mut self,
        hir: &HirFile,
        site: PatternSiteId,
        path: &hir::UnresolvedPath,
        fields: Option<&[hir::FieldPat]>,
        enum_def: DefId,
    ) {
        let Some((_, data)) = self.resolution.enum_of(enum_def) else {
            return;
        };
        let found = data
            .variants
            .iter()
            .position(|v| v.name == path.name)
            .map(|index| VariantRef {
                enum_def,
                index: u32::try_from(index).expect("variant count fits u32"),
            });
        match found {
            Some(variant) => {
                self.resolution
                    .uses
                    .insert(path.node, Res::Variant(variant));
                if let Some(fields) = fields {
                    self.resolve_fields(hir, site, fields, variant);
                }
            }
            None => {
                // Same-domain suggestions only: the candidates are this
                // enum's variants, never another enum's homonyms.
                let suggestion = {
                    let (_, data) = self.resolution.enum_of(enum_def).expect("still an enum");
                    nearest(&path.name, data.variants.iter().map(|v| v.name.as_str()))
                };
                self.resolution.uses.insert(path.node, Res::Unresolved);
                if let Some(suggestion) = suggestion {
                    self.resolution.unresolved.push(UnresolvedUse {
                        site,
                        node: path.node,
                        name: path.name.clone(),
                        kind: UseKind::Case,
                        against: enum_def,
                        tag: None,
                        suggestion,
                    });
                }
            }
        }
    }

    fn resolve_fields(
        &mut self,
        hir: &HirFile,
        site: PatternSiteId,
        fields: &[hir::FieldPat],
        variant: VariantRef,
    ) {
        let Some(declared) = self.resolution.variant(variant).and_then(|v| {
            v.fields.as_ref().map(|fields| {
                fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty_text.clone()))
                    .collect::<Vec<_>>()
            })
        }) else {
            // No field list to compare against (a unit case, or an import
            // that carried tags only) — nothing to say.
            return;
        };
        let variant_name = self
            .resolution
            .variant(variant)
            .map(|v| v.name.clone())
            .unwrap_or_default();
        for field_pat in fields {
            let found = declared
                .iter()
                .position(|(name, _)| *name == field_pat.name);
            match found {
                Some(index) => {
                    let field_ref = FieldRef {
                        variant,
                        index: u32::try_from(index).expect("field count fits u32"),
                    };
                    self.resolution
                        .uses
                        .insert(field_pat.node, Res::Field(field_ref));
                    // A nested pattern resolves against the field's own
                    // declared type, when that type names an enum this
                    // file can see.
                    if let FieldBinding::Nested(inner) = &field_pat.binding
                        && let Some(nested_enum) = self.enum_of_type(&declared[index].1)
                        && let Pat::Constructor { path, fields } = &hir.patterns[*inner]
                    {
                        self.resolve_constructor(hir, site, path, fields.as_deref(), nested_enum);
                    }
                }
                None => {
                    let suggestion = nearest(
                        &field_pat.name,
                        declared.iter().map(|(name, _)| name.as_str()),
                    );
                    self.resolution.uses.insert(field_pat.node, Res::Unresolved);
                    if let Some(suggestion) = suggestion {
                        self.resolution.unresolved.push(UnresolvedUse {
                            site,
                            node: field_pat.node,
                            name: field_pat.name.clone(),
                            kind: UseKind::Field,
                            against: variant.enum_def,
                            tag: Some(variant_name.clone()),
                            suggestion,
                        });
                    }
                }
            }
        }
    }

    /// The single-pattern near-miss licence: one tag is thin evidence, so
    /// an unidentified `if let`/let-else tag is reported only when exactly
    /// one enum has a case a *single* edit away (transposition included).
    fn report_near_miss(&mut self, hir: &HirFile, site: PatternSiteId, pattern: PatternId) {
        let Pat::Constructor { path, .. } = &hir.patterns[pattern] else {
            return;
        };
        let mut found: Option<(DefId, String)> = None;
        for (id, def) in self.resolution.defs.iter() {
            let DefKind::Enum(data) = &def.kind else {
                continue;
            };
            if self.resolution.type_ns.get(&def.name) != Some(&id) {
                continue;
            }
            let Some((suggestion, _)) =
                nearest_within(&path.name, data.variants.iter().map(|v| v.name.as_str()), 1)
            else {
                continue;
            };
            if found.is_some() {
                return; // ambiguous — an ambiguous suggestion is none
            }
            found = Some((id, suggestion));
        }
        if let Some((enum_def, suggestion)) = found {
            self.resolution.uses.insert(path.node, Res::Unresolved);
            self.resolution.unresolved.push(UnresolvedUse {
                site,
                node: path.node,
                name: path.name.clone(),
                kind: UseKind::Case,
                against: enum_def,
                tag: None,
                suggestion,
            });
        }
    }

    /// The enum a declared type text names — a bare (possibly dotted)
    /// identifier, optionally with type arguments; anything else (a union,
    /// an array…) names no single enum. Same rule as the analysis'.
    fn enum_of_type(&self, ty: &str) -> Option<DefId> {
        let trimmed = ty.trim();
        let base_len = trimmed
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.'))
            .count();
        if base_len == 0 {
            return None;
        }
        let rest = trimmed[base_len..].trim_start();
        let type_args = rest.starts_with('<') && rest.ends_with('>');
        if !rest.is_empty() && !type_args {
            return None;
        }
        self.resolution.type_ns.get(&trimmed[..base_len]).copied()
    }
}

/// The tags `pattern` contributes to `position`'s evidence. For a
/// single-subject site the whole pattern is position 0; for a tuple site
/// each tuple element speaks for its own position and a wildcard arm for
/// none.
fn collect_position_tags<'h>(
    hir: &'h HirFile,
    pattern: PatternId,
    position: usize,
    positions: usize,
    out: &mut Vec<&'h str>,
) {
    match &hir.patterns[pattern] {
        Pat::Wildcard | Pat::Literal(_) => {}
        Pat::Or(alts) => {
            for &alt in alts {
                collect_position_tags(hir, alt, position, positions, out);
            }
        }
        Pat::Tuple(elems) => {
            if positions > 1 {
                if let Some(&elem) = elems.get(position) {
                    collect_position_tags(hir, elem, 0, 1, out);
                }
            } else {
                for &elem in elems {
                    collect_position_tags(hir, elem, 0, 1, out);
                }
            }
        }
        // Top-level tags only: a nested pattern's tag is evidence about
        // the *payload*'s enum, not the subject's — same rule as the
        // analysis.
        Pat::Constructor { path, .. } => out.push(&path.name),
    }
}

/// `Option`/`Result` as declaration identities — the same shapes as the
/// standard library module and [`crate::analysis`]'s table.
fn builtin_enums() -> Vec<(String, Vec<VariantDecl>)> {
    let field = |name: &str, ty: &str| FieldDecl {
        name: name.to_string(),
        node: None,
        hir: None,
        optional: false,
        ty_text: ty.to_string(),
    };
    let unit = |name: &str| VariantDecl {
        name: name.to_string(),
        node: None,
        hir: None,
        fields: None,
    };
    let payload = |name: &str, fields: Vec<FieldDecl>| VariantDecl {
        name: name.to_string(),
        node: None,
        hir: None,
        fields: Some(fields),
    };
    vec![
        (
            "Option".to_string(),
            vec![payload("Some", vec![field("value", "T")]), unit("None")],
        ),
        (
            "Result".to_string(),
            vec![
                payload("Ok", vec![field("value", "T")]),
                payload("Err", vec![field("error", "E")]),
            ],
        ),
    ]
}

/// The declared name `written` looks like a misspelling of, if any — the
/// analysis' **licence to report** an unresolved name.
///
/// rlc does not know the scrutinee's type, so a name that resolves to
/// nothing is not by itself wrong: tag patterns match hand-written tagged
/// unions too, and their names are in no declaration table. What makes a
/// report safe is being able to say what to write instead, so this is the
/// whole rule — no suggestion, no error.
///
/// ```
/// // a real typo resolves; an unrelated name does not
/// let src = "enum Shape { Circle(radius: number), Empty }\n\
///            const v = match (s) { Circel(radius) => radius, Empty => 0 };\n";
/// let found = rlc::pattern_analyses(src, &[]);
/// assert_eq!(found.unresolved[0].name, "Circel");
/// assert_eq!(found.unresolved[0].suggestion, "Circle");
/// ```
fn nearest<'a>(written: &str, declared: impl Iterator<Item = &'a str>) -> Option<String> {
    nearest_within(written, declared, usize::MAX).map(|(name, _)| name)
}

/// [`nearest`] with an explicit ceiling on how far a name may be and still
/// count — and the distance, so a caller can weigh the evidence.
fn nearest_within<'a>(
    written: &str,
    declared: impl Iterator<Item = &'a str>,
    ceiling: usize,
) -> Option<(String, usize)> {
    let mut best: Option<(&str, usize)> = None;
    for name in declared {
        if name == written {
            return None;
        }
        let Some(distance) = typo_distance(written, name).filter(|d| *d <= ceiling) else {
            continue;
        };
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((name, distance));
        }
    }
    best.map(|(name, distance)| (name.to_string(), distance))
}

/// The edit distance between two names when it is small enough to call a
/// misspelling, `None` otherwise. Case alone always counts as one
/// (`circle` for `Circle`); otherwise the budget scales with the shorter
/// name, so short names — where every other name is "close" — are not
/// suggested for each other (`Ok` is never a typo of `Err`).
fn typo_distance(written: &str, declared: &str) -> Option<usize> {
    if written.eq_ignore_ascii_case(declared) {
        return Some(1);
    }
    let shortest = written.chars().count().min(declared.chars().count());
    let budget = match shortest {
        0..=2 => return None,
        3 => 1,
        _ => 2,
    };
    let distance = edit_distance(written, declared);
    (distance <= budget).then_some(distance)
}

/// Optimal string alignment distance over characters — Levenshtein plus
/// **transposition as a single edit**.
///
/// Transposing two letters (`Circel` for `Circle`) is the commonest typo
/// there is; counting it as one edit rather than two is what lets a
/// single-pattern site report it (see [`analyze_site`]).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut grid = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in grid.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in grid[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (grid[i - 1][j] + 1)
                .min(grid[i][j - 1] + 1)
                .min(grid[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(grid[i - 2][j - 2] + 1);
            }
            grid[i][j] = best;
        }
    }
    grid[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transposition_counts_as_one_edit() {
        // Which is what lets a single-pattern site report `Circel`.
        assert_eq!(edit_distance("Circel", "Circle"), 1);
        assert_eq!(edit_distance("Cyrcla", "Circle"), 2);
        assert_eq!(typo_distance("Ok", "Er"), None, "short names stay apart");
        assert_eq!(typo_distance("circle", "Circle"), Some(1), "case alone");
    }
}
