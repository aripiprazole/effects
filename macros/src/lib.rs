use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Abi, Attribute, Error, ExprParen, ExprYield, FnArg, Generics, Ident, PathArguments, ReturnType,
    Stmt, Token, Type, Visibility,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    spanned::Spanned as _,
    visit_mut::{self, VisitMut},
};

#[proc_macro_attribute]
pub fn suspend(_args: TokenStream, input: TokenStream) -> TokenStream {
    let suspend_fn = parse_macro_input!(input as SuspendFn);

    TokenStream::from(suspend_fn.into_token_stream())
}

struct ResumeEnum(Ident, Effects);

impl ResumeEnum {
    fn name(&self) -> Ident {
        format_ident!("__Resume_{}", self.0)
    }

    fn trait_name(&self) -> Ident {
        format_ident!("__Extract_{}", self.0)
    }
}

impl ToTokens for ResumeEnum {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let enum_name = self.name();
        let trait_name = self.trait_name();
        let mut entries = vec![];

        tokens.extend(quote! {
            #[allow(non_camel_case_types)]
            trait #trait_name: ::naloxone_frame::Effect {
                fn extract(r: #enum_name) -> <Self as ::naloxone_frame::Effect>::Resume;
            }
        });

        for (i, entry) in self.1.iter().enumerate() {
            let variant = variant_name(entry).unwrap_or(format_ident!("Resume{i}"));
            let resume_ty: Type = parse_quote!(<#entry as ::naloxone_frame::Effect>::Resume);
            entries.push(quote! {
                #variant(#resume_ty)
            });
            tokens.extend(quote! {
                impl #trait_name for #entry {
                    fn extract(r: #enum_name) -> <Self as ::naloxone_frame::Effect>::Resume {
                        match r {
                            #enum_name::#variant(v) => v,
                            _ => ::core::panic!("yield!: handler returned the wrong effect variant"),
                        }
                    }
                }
            });
        }

        tokens.extend(quote! {
            #[allow(non_camel_case_types)]
            pub enum #enum_name {
                Start,
                #(#entries),*
            }
        });
    }
}

struct YieldEnum(Ident, Effects);

impl YieldEnum {
    fn name(&self) -> Ident {
        format_ident!("__Yield_{}", self.0)
    }
}

impl ToTokens for YieldEnum {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let enum_name = self.name();
        let mut entries = vec![];
        for (i, entry) in self.1.iter().enumerate() {
            let variant = variant_name(entry).unwrap_or(format_ident!("Yield{i}"));
            entries.push(quote! {
                #variant(#entry)
            });
            tokens.extend(quote! {
                impl ::core::convert::From<#entry> for #enum_name {
                    fn from(node: #entry) -> Self {
                        Self::#variant(node)
                    }
                }
            });
        }
        tokens.extend(quote! {
            #[allow(non_camel_case_types)]
            pub enum #enum_name {
                #(#entries),*
            }
        });
    }
}

struct SuspendSignature {
    constness: Option<Token![const]>,
    asyncness: Option<Token![async]>,
    unsafety: Option<Token![unsafe]>,
    abi: Option<Abi>,
    ident: Ident,
    generics: Generics,
    inputs: Punctuated<FnArg, Token![,]>,
    return_type: Type,
}

struct SuspendBlock {
    stmts: Vec<Stmt>,
    yield_type: Ident,
    resume_type: Ident,
    extract_trait: Ident,
}

impl ToTokens for SuspendBlock {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let SuspendBlock {
            yield_type,
            resume_type,
            extract_trait,
            stmts,
        } = &self;
        let mut folder = YieldFolder {
            resume_type: resume_type.clone(),
            yield_type: yield_type.clone(),
            extract_trait: extract_trait.clone(),
        };
        let mut block = syn::Block {
            brace_token: Default::default(),
            stmts: stmts.clone(),
        };

        folder.visit_block_mut(&mut block);
        tokens.extend(quote! {
            #[coroutine] |_| #block
        });
    }
}

struct YieldFolder {
    resume_type: Ident,
    yield_type: Ident,
    extract_trait: Ident,
}

impl VisitMut for YieldFolder {
    fn visit_expr_mut(&mut self, node: &mut syn::Expr) {
        match node {
            syn::Expr::Paren(ExprParen { expr, .. }) => match &mut **expr {
                syn::Expr::Yield(ExprYield {
                    expr: Some(expr), ..
                }) => {
                    self.visit_expr_mut(&mut **expr);
                    let ref yield_type = self.yield_type;
                    let ref resume_type = self.resume_type;
                    let ref extract_trait = self.extract_trait;
                    *node = parse_quote!({
                        fn eqv<E>(eff: E) -> (#yield_type, impl FnOnce(#resume_type) -> <E as ::naloxone_frame::Effect>::Resume)
                        where E: #extract_trait,
                              E: ::core::convert::Into<#yield_type> {
                            (::core::convert::Into::into(eff), |output| E::extract(output))
                        }
                        let (eff, readback) = eqv(#expr);
                        let group = yield eff;
                        readback(group)
                    });
                }
                _ => visit_mut::visit_expr_mut(self, node),
            },
            syn::Expr::Yield(ExprYield {
                expr: Some(expr), ..
            }) => {
                self.visit_expr_mut(&mut **expr);
                let ref yield_type = self.yield_type;
                let ref resume_type = self.resume_type;
                let ref extract_trait = self.extract_trait;
                *node = parse_quote!({
                    fn eqv<E>(eff: E) -> (#yield_type, impl FnOnce(#resume_type) -> <E as ::naloxone_frame::Effect>::Resume)
                    where E: #extract_trait,
                          E: ::core::convert::Into<#yield_type> {
                        (::core::convert::Into::into(eff), |output| E::extract(output))
                    }
                    let (eff, readback) = eqv(#expr);
                    let group = yield eff;
                    readback(group)
                });
            }
            _ => visit_mut::visit_expr_mut(self, node),
        }
    }
}

struct SuspendFn {
    attrs: Vec<Attribute>,
    vis: Visibility,
    sig: SuspendSignature,
    block: SuspendBlock,
    yield_enum: YieldEnum,
    resume_enum: ResumeEnum,
}

impl ToTokens for SuspendFn {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let SuspendFn {
            attrs,
            vis,
            sig,
            block,
            yield_enum,
            resume_enum,
        } = &self;
        let SuspendSignature {
            constness,
            asyncness,
            unsafety,
            abi,
            ident,
            generics,
            inputs,
            return_type,
            ..
        } = &sig;
        let yield_enum_name = yield_enum.name();
        let resume_enum_name = resume_enum.name();
        tokens.extend(quote! {
            #yield_enum
            #resume_enum
            #(#attrs)*
            #vis
            #constness
            #asyncness
            #unsafety
            #abi
            fn #ident #generics (#inputs)
              -> impl ::std::ops::Coroutine<#resume_enum_name, Yield = #yield_enum_name, Return = #return_type> {
                  #block
              }
        });
    }
}

impl Parse for SuspendFn {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let actual = syn::ItemFn::parse(input)?;
        let (return_type, effects) = match actual.sig.output {
            ReturnType::Type(_, r#type) => parse_effects(*r#type)?,
            ReturnType::Default => {
                return Err(Error::new(
                    actual.sig.output.span(),
                    "expected Suspend<...> type",
                ));
            }
        };
        let yield_enum = YieldEnum(actual.sig.ident.clone(), effects.clone());
        let resume_enum = ResumeEnum(actual.sig.ident.clone(), effects.clone());
        Ok(Self {
            attrs: actual.attrs,
            vis: actual.vis,
            block: SuspendBlock {
                resume_type: resume_enum.name(),
                yield_type: yield_enum.name(),
                extract_trait: resume_enum.trait_name(),
                stmts: actual.block.stmts,
            },
            sig: SuspendSignature {
                constness: actual.sig.constness,
                asyncness: actual.sig.asyncness,
                unsafety: actual.sig.unsafety,
                abi: actual.sig.abi,
                ident: actual.sig.ident,
                generics: actual.sig.generics,
                inputs: actual.sig.inputs,
                return_type,
            },
            yield_enum,
            resume_enum,
        })
    }
}

#[derive(Clone)]
enum Effects {
    List(Punctuated<Type, Token![,]>),
    Single(Type),
}

impl Effects {
    fn iter(&self) -> impl Iterator<Item = &Type> {
        struct EffectsIter<'a> {
            inner: EffectsIterInner<'a>,
        }

        enum EffectsIterInner<'a> {
            List(syn::punctuated::Iter<'a, Type>),
            Single(std::iter::Once<&'a Type>),
        }

        impl<'a> Iterator for EffectsIter<'a> {
            type Item = &'a Type;

            fn next(&mut self) -> Option<Self::Item> {
                match &mut self.inner {
                    EffectsIterInner::List(iter) => iter.next(),
                    EffectsIterInner::Single(iter) => iter.next(),
                }
            }
        }

        EffectsIter {
            inner: match self {
                Effects::List(p) => EffectsIterInner::List(p.iter()),
                Effects::Single(t) => EffectsIterInner::Single(std::iter::once(t)),
            },
        }
    }
}

fn variant_name(r#type: &Type) -> Option<Ident> {
    match r#type {
        Type::Group(type_group) => variant_name(&*type_group.elem),
        Type::Paren(type_paren) => variant_name(&*type_paren.elem),
        Type::Path(type_path) => type_path.path.get_ident().cloned(),
        _ => None,
    }
}

fn parse_effects(r#type: Type) -> syn::Result<(Type, Effects)> {
    let span = r#type.span();
    let Type::Path(syn::TypePath { qself: None, path }) = r#type else {
        return Err(Error::new(
            span,
            "return type must be a path like Suspend<Resume, Effects>",
        ));
    };
    let segment = path
        .segments
        .last()
        .ok_or_else(|| Error::new(path.span(), "empty path"))?
        .clone();
    let PathArguments::AngleBracketed(args) = segment.arguments else {
        return Err(Error::new(
            segment.span(),
            "suspend must have generic arguments",
        ));
    };
    let mut iter = args.args.iter();
    let Some(syn::GenericArgument::Type(return_type)) = iter.next() else {
        return Err(Error::new(args.span(), "expected return type"));
    };
    let Some(syn::GenericArgument::Type(effects)) = iter.next() else {
        return Err(Error::new(args.span(), "expected effects type"));
    };
    Ok((
        return_type.clone(),
        match effects {
            Type::Tuple(tuple) => Effects::List(tuple.elems.clone()),
            other => Effects::Single(other.clone()),
        },
    ))
}
