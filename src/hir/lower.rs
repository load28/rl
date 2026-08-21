//! AST → HIR lowering.
//!
//! One pass over the parsed [`crate::ast::Program`], in source order. The
//! lowering is total (the parser is infallible, so there is nothing to
//! reject) and normalizing: the four pattern-carrying constructs become
//! [`PatternSite`]s, or-patterns become one [`Pat::Or`] node, passthrough
//! TypeScript stays an opaque span, and every node gets an ID plus a
//! source-map entry — the ID is the identity, the span is only a way back.

use super::*;
use crate::ast;

/// Lowers one file's source. The parse is the same infallible structural
/// parse the compiler runs; a file that is entirely TypeScript lowers to a
/// root body of opaque statements.
pub fn lower_source(file: FileId, source: &str) -> HirFile {
    let program = crate::parser::parse(source);
    lower_program(file, &program)
}

pub(crate) fn lower_program(file: FileId, program: &ast::Program) -> HirFile {
    let mut ctx = Lower {
        hir: HirFile {
            file,
            items: Vec::new(),
            variants: Arena::default(),
            fields: Arena::default(),
            bodies: Arena::default(),
            exprs: Arena::default(),
            patterns: Arena::default(),
            sites: Arena::default(),
            // Placeholder until the root body is lowered.
            root: BodyId(0),
            source_map: HirSourceMap::default(),
        },
        next_node: 0,
    };
    let root = ctx.lower_body(program);
    ctx.hir.root = root;
    ctx.hir
}

struct Lower {
    hir: HirFile,
    next_node: u32,
}

impl Lower {
    fn node(&mut self, span: Span, origin: AstOrigin) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        self.hir.source_map.record_node(id, span, origin);
        id
    }

    fn span(s: ast::Span) -> Span {
        Span::new(s.start, s.end)
    }

    /// A statement stream: one [`Body`] per `Program`.
    fn lower_body(&mut self, program: &ast::Program) -> BodyId {
        let mut stmts = Vec::with_capacity(program.segments.len());
        for segment in &program.segments {
            match segment {
                ast::Segment::Verbatim(span) => {
                    let node = self.node(Self::span(*span), AstOrigin::Verbatim);
                    stmts.push(Stmt::Opaque(node));
                }
                ast::Segment::ValModifier(span) => {
                    let node = self.node(Self::span(*span), AstOrigin::ValModifier);
                    stmts.push(Stmt::Opaque(node));
                }
                ast::Segment::Enum(decl) => {
                    let owner = self.lower_enum(decl);
                    stmts.push(Stmt::Item(owner));
                }
                ast::Segment::RlImport(decl) => {
                    let owner = self.lower_import(decl);
                    stmts.push(Stmt::Item(owner));
                }
                ast::Segment::Match(expr) => {
                    let id = self.lower_match(expr);
                    stmts.push(Stmt::Expr(id));
                }
                ast::Segment::TupleMatch(expr) => {
                    let id = self.lower_tuple_match(expr);
                    stmts.push(Stmt::Expr(id));
                }
                ast::Segment::Try(stmt) => stmts.push(Stmt::Try(self.lower_try(stmt))),
                ast::Segment::LetElse(stmt) => stmts.push(Stmt::LetElse(self.lower_let_else(stmt))),
                ast::Segment::IfLet(stmt) => stmts.push(Stmt::IfLet(self.lower_if_let(stmt))),
                ast::Segment::Template(template) => {
                    let id = self.lower_template(template);
                    stmts.push(Stmt::Expr(id));
                }
                ast::Segment::Pipe(pipe) => {
                    let id = self.lower_pipe(pipe);
                    stmts.push(Stmt::Expr(id));
                }
                ast::Segment::ResultBlock(block) => {
                    let id = self.lower_result_block(block);
                    stmts.push(Stmt::Expr(id));
                }
            }
        }
        self.hir.bodies.alloc(Body { stmts })
    }

    /// An expression position holding a `Program`: a single verbatim
    /// segment is the common case and stays one opaque span; anything
    /// richer (rl constructs inside the expression) becomes a [`Expr::Seq`]
    /// wrapping the lowered stream.
    fn lower_expr_program(&mut self, program: &ast::Program, span: Span) -> ExprId {
        if let [ast::Segment::Verbatim(inner)] = program.segments.as_slice() {
            let node = self.node(Self::span(*inner), AstOrigin::OpaqueExpr);
            return self.hir.exprs.alloc(Expr::OpaqueTs(node));
        }
        let node = self.node(span, AstOrigin::Seq);
        let body = self.lower_body(program);
        self.hir.exprs.alloc(Expr::Seq { node, body })
    }

    fn lower_enum(&mut self, decl: &ast::EnumDecl) -> OwnerId {
        let owner = OwnerId(u32::try_from(self.hir.items.len()).expect("item count fits u32"));
        let node = self.node(
            Span::new(decl.name_off, decl.name_off + decl.name.len()),
            AstOrigin::EnumDecl,
        );
        let mut variants = Vec::with_capacity(decl.cases.len());
        for case in &decl.cases {
            let case_node = self.node(
                Span::new(case.tag_off, case.tag_off + case.tag.len()),
                AstOrigin::EnumCase,
            );
            // The variant is allocated before its fields so the owner link
            // can be recorded on each field.
            let variant = self.hir.variants.alloc(VariantData {
                node: case_node,
                owner,
                name: case.tag.clone(),
                fields: None,
            });
            let fields = case.fields.as_ref().map(|fields| {
                fields
                    .iter()
                    .map(|field| {
                        let field_node = self.node(
                            Span::new(field.name_off, field.name_off + field.name.len()),
                            AstOrigin::EnumField,
                        );
                        self.hir.fields.alloc(FieldData {
                            node: field_node,
                            owner: variant,
                            name: field.name.clone(),
                            optional: field.optional,
                            ty_text: field.ty.clone(),
                        })
                    })
                    .collect()
            });
            self.hir.variants[variant].fields = fields;
            variants.push(variant);
        }
        self.hir.items.push(Item::Enum(EnumItem {
            node,
            name: decl.name.clone(),
            exported: decl.exported,
            generics: decl.generics.clone(),
            variants,
        }));
        owner
    }

    fn lower_import(&mut self, decl: &ast::RlImportDecl) -> OwnerId {
        let owner = OwnerId(u32::try_from(self.hir.items.len()).expect("item count fits u32"));
        let node = self.node(Self::span(decl.spec), AstOrigin::Import);
        self.hir.items.push(Item::Import(ImportItem {
            node,
            kind: match decl.kind {
                ast::RlSpecifier::Relative => ImportKind::Relative,
                ast::RlSpecifier::Std => ImportKind::Std,
            },
            names: match &decl.names {
                ast::RlImportNames::Namespace(ns) => ImportNames::Namespace(ns.clone()),
                ast::RlImportNames::Named(entries) => ImportNames::Named(entries.clone()),
                ast::RlImportNames::None => ImportNames::None,
            },
        }));
        owner
    }

    fn lower_match(&mut self, expr: &ast::MatchExpr) -> ExprId {
        let head = Span::new(expr.keyword_off, expr.scrutinee_span.end + 1);
        let node = self.node(head, AstOrigin::Match);
        let subject = self.lower_expr_program(&expr.scrutinee, Self::span(expr.scrutinee_span));
        let arms = expr
            .arms
            .iter()
            .map(|arm| self.lower_arm(&arm.pattern, arm.pattern_span, &arm.guard, Some(&arm.body)))
            .collect();
        let site_node = self.node(head, AstOrigin::Match);
        let site = self.hir.sites.alloc(PatternSite {
            node: site_node,
            kind: SiteKind::Match,
            subjects: vec![subject],
            arms,
        });
        self.hir.exprs.alloc(Expr::Match { node, site })
    }

    fn lower_tuple_match(&mut self, expr: &ast::TupleMatchExpr) -> ExprId {
        let head_end = expr
            .scrutinees
            .last()
            .map_or(expr.keyword_off, |(span, _)| span.end + 1);
        let head = Span::new(expr.keyword_off, head_end);
        let node = self.node(head, AstOrigin::TupleMatch);
        let subjects = expr
            .scrutinees
            .iter()
            .map(|(span, program)| self.lower_expr_program(program, Self::span(*span)))
            .collect();
        let arms = expr
            .arms
            .iter()
            .map(|arm| {
                let pattern = match &arm.pattern {
                    ast::TuplePattern::Wildcard => {
                        self.alloc_pattern(Pat::Wildcard, Self::span(arm.pattern_span))
                    }
                    ast::TuplePattern::Elems(elems) => {
                        let lowered: Vec<PatternId> = elems
                            .iter()
                            .map(|elem| self.lower_pattern(elem, arm.pattern_span.start))
                            .collect();
                        self.alloc_pattern(Pat::Tuple(lowered), Self::span(arm.pattern_span))
                    }
                };
                let arm_node = self.node(
                    Span::new(arm.pattern_span.start, arm.body_span.end),
                    AstOrigin::Arm,
                );
                let guard = arm.guard.as_ref().map(|g| self.lower_guard(g));
                let body = self.lower_body(&arm.body);
                SiteArm {
                    node: arm_node,
                    pattern,
                    guard,
                    body: Some(body),
                }
            })
            .collect();
        let site_node = self.node(head, AstOrigin::TupleMatch);
        let site = self.hir.sites.alloc(PatternSite {
            node: site_node,
            kind: SiteKind::TupleMatch,
            subjects,
            arms,
        });
        self.hir.exprs.alloc(Expr::Match { node, site })
    }

    fn lower_arm(
        &mut self,
        pattern: &ast::Pattern,
        pattern_span: ast::Span,
        guard: &Option<ast::GuardExpr>,
        body: Option<&ast::Program>,
    ) -> SiteArm {
        let pattern_id = self.lower_pattern(pattern, pattern_span.start);
        let node = self.node(Self::span(pattern_span), AstOrigin::Arm);
        let guard = guard.as_ref().map(|g| self.lower_guard(g));
        let body = body.map(|b| self.lower_body(b));
        SiteArm {
            node,
            pattern: pattern_id,
            guard,
            body,
        }
    }

    fn lower_guard(&mut self, guard: &ast::GuardExpr) -> ExprId {
        self.lower_expr_program(&guard.expr, Self::span(guard.span))
    }

    fn alloc_pattern(&mut self, pat: Pat, span: Span) -> PatternId {
        let id = self.hir.patterns.alloc(pat);
        self.hir.source_map.record_pattern(id, span);
        id
    }

    /// One arm-level pattern: a wildcard, a group of tag alternatives, or
    /// a group of literal alternatives. A single alternative is that
    /// pattern itself; several become one [`Pat::Or`].
    fn lower_pattern(&mut self, pattern: &ast::Pattern, pattern_off: usize) -> PatternId {
        match pattern {
            ast::Pattern::Wildcard => {
                self.alloc_pattern(Pat::Wildcard, Span::new(pattern_off, pattern_off + 1))
            }
            ast::Pattern::Tags(alts) => {
                let lowered: Vec<PatternId> =
                    alts.iter().map(|alt| self.lower_tag_pattern(alt)).collect();
                self.or_of(lowered)
            }
            ast::Pattern::Literals(alts) => {
                let lowered: Vec<PatternId> = alts
                    .iter()
                    .map(|alt| {
                        self.alloc_pattern(
                            Pat::Literal(match &alt.value {
                                ast::LiteralValue::Str(s) => LitValue::Str(s.clone()),
                                ast::LiteralValue::Num(n) => LitValue::Num(*n),
                                ast::LiteralValue::BigInt(d) => LitValue::BigInt(d.clone()),
                                ast::LiteralValue::Bool(b) => LitValue::Bool(*b),
                            }),
                            Self::span(alt.span),
                        )
                    })
                    .collect();
                self.or_of(lowered)
            }
        }
    }

    fn or_of(&mut self, mut alternatives: Vec<PatternId>) -> PatternId {
        if alternatives.len() == 1 {
            return alternatives.remove(0);
        }
        let start = alternatives
            .first()
            .and_then(|p| self.hir.source_map.pattern_span(*p))
            .map_or(0, |s| s.start);
        let end = alternatives
            .last()
            .and_then(|p| self.hir.source_map.pattern_span(*p))
            .map_or(start, |s| s.end);
        self.alloc_pattern(Pat::Or(alternatives), Span::new(start, end))
    }

    fn lower_tag_pattern(&mut self, alt: &ast::TagPattern) -> PatternId {
        let path_node = self.node(
            Span::new(alt.tag_off, alt.tag_off + alt.tag.len()),
            AstOrigin::Pattern,
        );
        let fields = alt.bindings.as_ref().map(|bindings| {
            bindings
                .iter()
                .map(|binding| self.lower_field_pat(binding))
                .collect()
        });
        self.alloc_pattern(
            Pat::Constructor {
                path: UnresolvedPath {
                    node: path_node,
                    name: alt.tag.clone(),
                },
                fields,
            },
            Span::new(alt.tag_off, alt.end),
        )
    }

    fn lower_field_pat(&mut self, binding: &ast::Binding) -> FieldPat {
        let node = self.node(Self::span(binding.name_span), AstOrigin::PatternField);
        let field_binding = match &binding.nested {
            Some(inner) => FieldBinding::Nested(self.lower_tag_pattern(inner)),
            None => FieldBinding::Named {
                alias: binding.alias.as_ref().map(|alias| {
                    let span = binding
                        .alias_span
                        .map(Self::span)
                        .unwrap_or(Self::span(binding.name_span));
                    (alias.clone(), self.node(span, AstOrigin::PatternField))
                }),
            },
        };
        FieldPat {
            node,
            name: binding.name.clone(),
            binding: field_binding,
        }
    }

    fn lower_try(&mut self, stmt: &ast::TryStmt) -> TryStmt {
        let node = self.node(Self::span(stmt.span), AstOrigin::Try);
        let binding = stmt
            .decl
            .as_ref()
            .map(|(_, span)| self.node(Self::span(*span), AstOrigin::BindingText));
        let expr = self.lower_expr_program(&stmt.expr, Self::span(stmt.span));
        TryStmt {
            node,
            binding,
            expr,
        }
    }

    fn lower_let_else(&mut self, stmt: &ast::LetElseStmt) -> LetElseStmt {
        let node = self.node(Self::span(stmt.head_span), AstOrigin::LetElse);
        // The let-else pattern's alternatives are lowered through the same
        // path as a match arm's ([`Pat::Or`] for several), so it is the
        // same shape to every analysis.
        let lowered: Vec<PatternId> = stmt
            .alternatives
            .iter()
            .map(|alt| self.lower_tag_pattern(alt))
            .collect();
        let pattern = self.or_of(lowered);
        let pattern_span = self
            .hir
            .source_map
            .pattern_span(pattern)
            .unwrap_or(Self::span(stmt.head_span));
        let arm_node = self.node(pattern_span, AstOrigin::Arm);
        let subject = self.lower_expr_program(&stmt.expr, Self::span(stmt.head_span));
        let site_node = self.node(Self::span(stmt.head_span), AstOrigin::LetElse);
        let site = self.hir.sites.alloc(PatternSite {
            node: site_node,
            kind: SiteKind::LetElse,
            subjects: vec![subject],
            arms: vec![SiteArm {
                node: arm_node,
                pattern,
                guard: None,
                // The bindings flow into the statements after the
                // construct; there is no body that runs "on match".
                body: None,
            }],
        });
        let else_body = self.lower_body(&stmt.else_body);
        LetElseStmt {
            node,
            site,
            else_body,
            else_diverges_hint: stmt.diverges,
        }
    }

    fn lower_if_let(&mut self, stmt: &ast::IfLetStmt) -> IfLetStmt {
        let node = self.node(Self::span(stmt.head_span), AstOrigin::IfLet);
        let lowered: Vec<PatternId> = stmt
            .alternatives
            .iter()
            .map(|alt| self.lower_tag_pattern(alt))
            .collect();
        let pattern = self.or_of(lowered);
        let pattern_span = self
            .hir
            .source_map
            .pattern_span(pattern)
            .unwrap_or(Self::span(stmt.head_span));
        let arm_node = self.node(pattern_span, AstOrigin::Arm);
        let subject = self.lower_expr_program(&stmt.expr, Self::span(stmt.head_span));
        let body = self.lower_body(&stmt.body);
        let site_node = self.node(Self::span(stmt.head_span), AstOrigin::IfLet);
        let site = self.hir.sites.alloc(PatternSite {
            node: site_node,
            kind: SiteKind::IfLet,
            subjects: vec![subject],
            arms: vec![SiteArm {
                node: arm_node,
                pattern,
                guard: None,
                body: Some(body),
            }],
        });
        let else_part = stmt.else_part.as_ref().map(|else_part| match else_part {
            ast::IfLetElse::Block(block) => IfLetElse::Block(self.lower_body(block)),
            ast::IfLetElse::IfLet(inner) => IfLetElse::IfLet(Box::new(self.lower_if_let(inner))),
        });
        IfLetStmt {
            node,
            site,
            else_part,
        }
    }

    fn lower_template(&mut self, template: &ast::Template) -> ExprId {
        // The template's own extent: from its first raw chunk to its last.
        let (start, end) = template
            .chunks
            .iter()
            .filter_map(|chunk| match chunk {
                ast::TemplateChunk::Raw(span) => Some((span.start, span.end)),
                ast::TemplateChunk::Interp(_) => None,
            })
            .fold(None, |acc: Option<(usize, usize)>, (s, e)| match acc {
                Some((min, max)) => Some((min.min(s), max.max(e))),
                None => Some((s, e)),
            })
            .unwrap_or((0, 0));
        let node = self.node(Span::new(start, end), AstOrigin::Template);
        let interps = template
            .chunks
            .iter()
            .filter_map(|chunk| match chunk {
                ast::TemplateChunk::Interp(program) => {
                    Some(self.lower_expr_program(program, Span::new(start, end)))
                }
                ast::TemplateChunk::Raw(_) => None,
            })
            .collect();
        self.hir.exprs.alloc(Expr::Template { node, interps })
    }

    fn lower_pipe(&mut self, pipe: &ast::PipeExpr) -> ExprId {
        let node = self.node(Self::span(pipe.head_span), AstOrigin::Pipe);
        let head = pipe
            .head
            .as_ref()
            .map(|head| self.lower_expr_program(head, Self::span(pipe.head_span)));
        let steps = pipe
            .steps
            .iter()
            .map(|step| {
                let step_node = self.node(Self::span(step.span), AstOrigin::PipeStep);
                let body = self.lower_expr_program(&step.body, Self::span(step.span));
                PipeStep {
                    node: step_node,
                    postfix: step.postfix,
                    body,
                }
            })
            .collect();
        self.hir.exprs.alloc(Expr::Pipe { node, head, steps })
    }

    fn lower_result_block(&mut self, block: &ast::ResultBlock) -> ExprId {
        let node = self.node(
            Span::new(block.keyword_off, block.body_span.end),
            AstOrigin::ResultBlock,
        );
        let items = block
            .items
            .iter()
            .map(|item| match item {
                ast::ResultItem::Stmts(stmts) => ResultItem::Stmts(self.lower_body(stmts)),
                ast::ResultItem::Bind(bind) => {
                    let bind_node = self.node(
                        Span::new(bind.binding_span.start, bind.expr_span.end),
                        AstOrigin::ResultBind,
                    );
                    let binding = self.node(Self::span(bind.binding_span), AstOrigin::BindingText);
                    let expr = self.lower_expr_program(&bind.expr, Self::span(bind.expr_span));
                    ResultItem::Bind {
                        node: bind_node,
                        binding,
                        expr,
                    }
                }
            })
            .collect();
        let value = self.lower_expr_program(&block.value, Self::span(block.body_span));
        self.hir
            .exprs
            .alloc(Expr::ResultBlock { node, items, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> HirFile {
        lower_source(FileId(0), source)
    }

    fn text(source: &str, span: Span) -> &str {
        &source[span.start..span.end]
    }

    /// The site's one constructor pattern, however the construct spelled it.
    fn only_constructor(hir: &HirFile, site: PatternSiteId) -> &Pat {
        let arm = &hir.sites[site].arms[0];
        &hir.patterns[arm.pattern]
    }

    #[test]
    fn a_plain_typescript_file_lowers_to_opaque_statements() {
        let src = "const n: number = 1;\nexport function f() { return n; }\n";
        let hir = lower(src);
        let root = &hir.bodies[hir.root];
        assert_eq!(root.stmts.len(), 1);
        let Stmt::Opaque(node) = &root.stmts[0] else {
            panic!("expected one opaque statement, got {root:?}");
        };
        assert_eq!(text(src, hir.source_map.node_span(*node).unwrap()), src);
        assert_eq!(hir.source_map.ast_origin(*node), Some(AstOrigin::Verbatim));
        assert!(hir.sites.is_empty());
        assert!(hir.items.is_empty());
    }

    #[test]
    fn an_enum_declaration_collects_variants_and_fields_with_identity() {
        let src = "export enum Shape { Circle(radius: number), Point }\n";
        let hir = lower(src);
        let [Item::Enum(decl)] = hir.items.as_slice() else {
            panic!("expected one enum item, got {:?}", hir.items);
        };
        assert_eq!(decl.name, "Shape");
        assert!(decl.exported);
        assert_eq!(decl.variants.len(), 2);
        assert_eq!(
            text(src, hir.source_map.node_span(decl.node).unwrap()),
            "Shape"
        );

        let circle = &hir.variants[decl.variants[0]];
        assert_eq!(circle.name, "Circle");
        assert_eq!(circle.owner, OwnerId(0));
        assert_eq!(
            text(src, hir.source_map.node_span(circle.node).unwrap()),
            "Circle"
        );
        let fields = circle.fields.as_ref().expect("Circle has a payload");
        let radius = &hir.fields[fields[0]];
        assert_eq!(radius.name, "radius");
        assert_eq!(radius.ty_text, "number");
        assert_eq!(radius.owner, decl.variants[0]);
        assert_eq!(
            text(src, hir.source_map.node_span(radius.node).unwrap()),
            "radius"
        );

        let point = &hir.variants[decl.variants[1]];
        assert_eq!(point.name, "Point");
        assert!(point.fields.is_none());
    }

    #[test]
    fn every_pattern_syntax_lowers_to_the_same_site_shape() {
        let src = "enum E { A(x: number), B }\n\
            const m = match (v) { A(x) => x, B => 0 };\n\
            if let A(x) = (v) {\n  use(x);\n}\n\
            const A(x2) = w else { return; };\n";
        let hir = lower(src);
        assert_eq!(hir.sites.len(), 3);
        let kinds: Vec<SiteKind> = hir.sites.iter().map(|(_, s)| s.kind).collect();
        assert_eq!(kinds, [SiteKind::Match, SiteKind::IfLet, SiteKind::LetElse]);
        for (_, site) in hir.sites.iter() {
            assert_eq!(site.subjects.len(), 1);
            assert!(!site.arms.is_empty());
        }
        // All three constructs' patterns are the same constructor shape.
        for (id, site) in hir.sites.iter() {
            let Pat::Constructor { path, .. } = only_constructor(&hir, id) else {
                panic!("site {:?} first pattern is not a constructor", site.kind);
            };
            assert_eq!(path.name, "A");
            assert_eq!(text(src, hir.source_map.node_span(path.node).unwrap()), "A");
        }
    }

    #[test]
    fn an_or_pattern_is_one_node_with_alternatives() {
        let src = "enum E { A(x: number), B(x: number), C }\n\
            const m = match (v) { A(x) | B(x) => x, C => 0 };\n";
        let hir = lower(src);
        let site = hir.sites.iter().next().unwrap().1;
        let Pat::Or(alts) = &hir.patterns[site.arms[0].pattern] else {
            panic!("expected an or-pattern");
        };
        assert_eq!(alts.len(), 2);
        for alt in alts {
            let Pat::Constructor { fields, .. } = &hir.patterns[*alt] else {
                panic!("or-alternative is not a constructor");
            };
            let fields = fields.as_ref().unwrap();
            assert_eq!(fields[0].name, "x");
        }
        // The or-group's span covers both alternatives.
        let span = hir.source_map.pattern_span(site.arms[0].pattern).unwrap();
        assert_eq!(text(src, span), "A(x) | B(x)");
    }

    #[test]
    fn a_nested_pattern_recurses_through_the_field() {
        let src = "const m = match (r) { Ok(value: Some(v)) => v, _ => 0 };\n";
        let hir = lower(src);
        let site = hir.sites.iter().next().unwrap().1;
        let Pat::Constructor { path, fields } = &hir.patterns[site.arms[0].pattern] else {
            panic!("expected a constructor");
        };
        assert_eq!(path.name, "Ok");
        let fields = fields.as_ref().unwrap();
        assert_eq!(fields[0].name, "value");
        let FieldBinding::Nested(inner) = &fields[0].binding else {
            panic!("expected a nested pattern");
        };
        let Pat::Constructor {
            path: inner_path, ..
        } = &hir.patterns[*inner]
        else {
            panic!("nested pattern is not a constructor");
        };
        assert_eq!(inner_path.name, "Some");
        // The wildcard arm lowered too.
        assert!(matches!(hir.patterns[site.arms[1].pattern], Pat::Wildcard));
    }

    #[test]
    fn a_tuple_match_lowers_positions_and_tuple_patterns() {
        let src = "enum A { X, Y }\nenum B { P, Q }\n\
            const m = match (a, b) { (X, P) => 0, (Y, Q(v)) => v, _ => 1 };\n";
        let hir = lower(src);
        let site = hir
            .sites
            .iter()
            .find(|(_, s)| s.kind == SiteKind::TupleMatch)
            .unwrap()
            .1;
        assert_eq!(site.subjects.len(), 2);
        let Pat::Tuple(elems) = &hir.patterns[site.arms[0].pattern] else {
            panic!("expected a tuple pattern");
        };
        assert_eq!(elems.len(), 2);
        assert!(matches!(hir.patterns[site.arms[2].pattern], Pat::Wildcard));
    }

    #[test]
    fn literals_and_guards_lower_with_their_values() {
        let src = "const m = match (code) { 200 | 201 => \"ok\", n if n >= 500 => \"err\", _ => \"other\" };\n";
        let hir = lower(src);
        let site = hir.sites.iter().next().unwrap().1;
        let Pat::Or(alts) = &hir.patterns[site.arms[0].pattern] else {
            panic!("expected an or of literals");
        };
        assert_eq!(alts.len(), 2);
        assert!(matches!(
            hir.patterns[alts[0]],
            Pat::Literal(LitValue::Num(n)) if n == 200.0
        ));
        // The guarded arm carries its guard as an expression.
        assert!(site.arms[1].guard.is_some());
    }

    #[test]
    fn try_and_result_record_bindings_and_single_evaluation_shape() {
        let src = "function f() {\n  const a = try readNum();\n  const r = result {\n    const b <- parse(a);\n    b\n  };\n  return r;\n}\n";
        let hir = lower(src);
        // The try and the result block are nested inside the function's
        // verbatim text? No: try/let-else are only claimed in statement
        // streams the parser walks — which includes function bodies. Find
        // them wherever they landed.
        let mut tries = 0;
        let mut binds = 0;
        for (_, body) in hir.bodies.iter() {
            for stmt in &body.stmts {
                if let Stmt::Try(t) = stmt {
                    tries += 1;
                    assert!(t.binding.is_some(), "declaration form keeps its binding");
                    let span = hir.source_map.node_span(t.node).unwrap();
                    assert_eq!(text(src, span), "try readNum()");
                }
            }
        }
        for (_, expr) in hir.exprs.iter() {
            if let Expr::ResultBlock { items, .. } = expr {
                for item in items {
                    if let ResultItem::Bind { binding, .. } = item {
                        binds += 1;
                        let span = hir.source_map.node_span(*binding).unwrap();
                        assert_eq!(text(src, span), "b");
                    }
                }
            }
        }
        assert_eq!((tries, binds), (1, 1));
    }

    #[test]
    fn ids_are_identity_and_spans_are_only_a_way_back() {
        // Two matches with byte-identical text: same spelling, different
        // identity — the sites and their patterns are distinct IDs even
        // though every string in them compares equal.
        let src = "enum E { A, B }\n\
            const m1 = match (v) { A => 0, B => 1 };\n\
            const m2 = match (v) { A => 0, B => 1 };\n";
        let hir = lower(src);
        assert_eq!(hir.sites.len(), 2);
        let ids: Vec<PatternSiteId> = hir.sites.iter().map(|(id, _)| id).collect();
        assert_ne!(ids[0], ids[1]);
        let first = &hir.sites[ids[0]];
        let second = &hir.sites[ids[1]];
        assert_ne!(first.arms[0].pattern, second.arms[0].pattern);
        // …and each resolves to its own source position.
        let a = hir.source_map.node_span(first.node).unwrap();
        let b = hir.source_map.node_span(second.node).unwrap();
        assert_ne!(a, b);
        assert_eq!(text(src, a), "match (v)");
        assert_eq!(text(src, b), "match (v)");
    }

    #[test]
    fn else_chains_and_bodies_keep_their_streams() {
        let src = "if let A(x) = v {\n  one(x);\n} else if let B(y) = v {\n  two(y);\n} else {\n  three();\n}\n";
        let hir = lower(src);
        let root = &hir.bodies[hir.root];
        let if_let = root
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::IfLet(stmt) => Some(stmt),
                _ => None,
            })
            .expect("an if let lowered");
        let Some(IfLetElse::IfLet(chained)) = &if_let.else_part else {
            panic!("expected a chained else-if-let");
        };
        assert!(matches!(chained.else_part, Some(IfLetElse::Block(_))));
        // Both sites exist and both arms carry then-bodies.
        assert_eq!(hir.sites.len(), 2);
        for (_, site) in hir.sites.iter() {
            assert_eq!(site.kind, SiteKind::IfLet);
            assert!(site.arms[0].body.is_some());
        }
    }
}
