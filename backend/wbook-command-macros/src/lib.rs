use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2, TokenTree};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, visit::Visit, FnArg, GenericArgument, Item, ItemFn, ItemMod, Pat,
    PathArguments, ReturnType, Type,
};

#[proc_macro_attribute]
pub fn unified_commands(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return syn::Error::new_spanned(
            TokenStream2::from(attribute),
            "unified_commands takes no arguments",
        )
        .into_compile_error()
        .into();
    }
    expand(parse_macro_input!(item as ItemMod))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(mut module: ItemMod) -> syn::Result<TokenStream2> {
    let wrapper_module = fresh_identifier("__wbook_tauri_commands", quote!(#module));
    let (_, items) = module.content.take().ok_or_else(|| {
        syn::Error::new_spanned(&module, "unified_commands requires an inline module")
    })?;
    let mut declarations = Vec::new();
    let mut wrappers = Vec::new();
    let mut commands = Vec::new();
    let mut queries = Vec::new();
    let mut mutations = Vec::new();
    let mut shared = Vec::new();
    let mut desktop = Vec::new();
    for item in items {
        match item {
            Item::Use(item) => declarations.push(quote!(#item)),
            Item::Fn(mut function) => {
                validate_function(&function)?;
                let name = function.sig.ident.clone();
                if commands.contains(&name) {
                    return Err(syn::Error::new_spanned(name, "duplicate command name"));
                }
                let desktop_only = take_marker(&mut function, "desktop_only")?;
                if take_marker(&mut function, "query")? {
                    queries.push(name.clone());
                } else {
                    mutations.push(name.clone());
                }
                if desktop_only {
                    desktop.push(name.clone());
                    wrappers.push(expand_desktop(&function));
                } else {
                    let (checked, wrapper) = expand_shared(&function)?;
                    declarations.push(checked);
                    wrappers.push(wrapper);
                    shared.push(name.clone());
                }
                declarations.push(quote!(#function));
                commands.push(name);
            }
            item => {
                return Err(syn::Error::new_spanned(
                    item,
                    "command modules support only use declarations and command functions",
                ))
            }
        }
    }
    if commands.is_empty() {
        return Err(syn::Error::new_spanned(
            &module,
            "command modules must contain at least one command",
        ));
    }
    let desktop_names: Vec<_> = desktop.iter().map(ToString::to_string).collect();
    let shared_names: Vec<_> = shared.iter().map(ToString::to_string).collect();
    let attributes = &module.attrs;
    let visibility = &module.vis;
    let name = &module.ident;
    Ok(quote! {
        #(#attributes)*
        #visibility mod #name {
            #(#declarations)*

            mod #wrapper_module {
                use super::*;
                #(#wrappers)*
            }

            pub const DESKTOP_ONLY_COMMANDS: &[&str] = &[#(#desktop_names),*];

            /// Returns the TanStack Query helpers to export with `Typescript::with_raw`
            /// together with the builder that owns the invoke handler.
            pub fn builder<R: ::tauri::Runtime>() -> (String, ::tauri_specta::Builder<R>) {
                let (queries, builder) = ::tauri_specta_query::CommandSet::new(
                    ::tauri_specta::collect_commands![#(#wrapper_module::#queries),*],
                    ::tauri_specta::collect_commands![#(#wrapper_module::#mutations),*],
                )
                .constant("DESKTOP_ONLY_COMMANDS", DESKTOP_ONLY_COMMANDS)
                .build(::tauri_specta_query::TanstackQueryFramework::React);
                // Query functions must reject so TanStack Query reports failures as errors.
                let builder = builder
                    .error_handling(::tauri_specta::ErrorHandlingMode::Throw)
                    .dangerously_cast_bigints_to_number();
                (queries, builder)
            }

            pub async fn dispatch(
                app: &::wbook_core::Wbook,
                method: &str,
                params: ::serde_json::Value,
            ) -> Result<::serde_json::Value, crate::commands::dto::CommandError> {
                match method {
                    #(#shared_names => #shared::rpc(app, params).await,)*
                    name if DESKTOP_ONLY_COMMANDS.contains(&name) => Err(crate::commands::dto::CommandError::new(
                        crate::commands::dto::ErrorKind::PlatformUnsupported,
                        "This operation requires the desktop application",
                    )),
                    _ => Err(crate::commands::dto::CommandError::new(
                        crate::commands::dto::ErrorKind::MethodNotFound,
                        "Unknown command",
                    )),
                }
            }
        }
    })
}

fn fresh_identifier(prefix: &str, input: TokenStream2) -> Ident {
    fn contains(input: TokenStream2, name: &str) -> bool {
        input.into_iter().any(|token| match token {
            TokenTree::Ident(ident) => ident.to_string().trim_start_matches("r#") == name,
            TokenTree::Group(group) => contains(group.stream(), name),
            _ => false,
        })
    }
    let mut name = prefix.to_owned();
    while contains(input.clone(), &name) {
        name.push('_');
    }
    format_ident!("{name}")
}

fn take_marker(function: &mut ItemFn, marker: &str) -> syn::Result<bool> {
    let mut found = false;
    for attribute in &function.attrs {
        if attribute.path().is_ident(marker) {
            if found || !matches!(attribute.meta, syn::Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    attribute,
                    format!("use {marker} once without arguments"),
                ));
            }
            found = true;
        }
    }
    function
        .attrs
        .retain(|attribute| !attribute.path().is_ident(marker));
    Ok(found)
}

fn expand_desktop(function: &ItemFn) -> TokenStream2 {
    let name = &function.sig.ident;
    let attributes = &function.attrs;
    let mut signature = function.sig.clone();
    let arguments: Vec<_> = signature
        .inputs
        .iter_mut()
        .map(|argument| {
            let FnArg::Typed(argument) = argument else {
                unreachable!()
            };
            let Pat::Ident(pattern) = argument.pat.as_mut() else {
                unreachable!()
            };
            pattern.mutability = None;
            pattern.ident.clone()
        })
        .collect();
    let awaited = signature.asyncness.map(|_| quote!(.await));
    quote! {
        #(#attributes)*
        #[::tauri::command]
        #[::specta::specta]
        pub(super) #signature {
            super::#name(#(#arguments),*) #awaited
        }
    }
}

fn validate_function(function: &ItemFn) -> syn::Result<()> {
    let signature = &function.sig;
    if signature.constness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "commands cannot be const, unsafe, extern, or variadic",
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "generic commands are not supported",
        ));
    }
    if ["builder", "dispatch", "DESKTOP_ONLY_COMMANDS"]
        .contains(&signature.ident.to_string().as_str())
        || signature.ident.to_string().starts_with("r#")
    {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            "this command name is reserved or uses a raw identifier",
        ));
    }
    for attribute in &function.attrs {
        if attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr") {
            return Err(syn::Error::new_spanned(
                attribute,
                "conditional command declarations are not supported",
            ));
        }
    }
    for argument in &signature.inputs {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "command methods with self are not supported",
            ));
        };
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "command arguments must use identifier patterns",
            ));
        };
        if pattern.by_ref.is_some() || pattern.subpat.is_some() {
            return Err(syn::Error::new_spanned(
                pattern,
                "command arguments must use plain identifier patterns",
            ));
        }
    }
    Ok(())
}

fn expand_shared(function: &ItemFn) -> syn::Result<(TokenStream2, TokenStream2)> {
    let signature = &function.sig;
    let Some(FnArg::Typed(service)) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            signature,
            "shared commands require an initial &Wbook service argument",
        ));
    };
    let valid_service = match service.ty.as_ref() {
        Type::Reference(reference)
            if reference.mutability.is_none() && reference.lifetime.is_none() =>
        {
            matches!(reference.elem.as_ref(), Type::Path(path) if path.qself.is_none() && path.path.segments.last().is_some_and(|segment| segment.ident == "Wbook" && matches!(segment.arguments, PathArguments::None)))
        }
        _ => false,
    };
    if !valid_service {
        return Err(syn::Error::new_spanned(
            &service.ty,
            "the first argument must be an immutable &Wbook service",
        ));
    }
    let ReturnType::Type(_, result_type) = &signature.output else {
        return Err(syn::Error::new_spanned(
            signature,
            "shared commands must return Result<T, CommandError>",
        ));
    };
    let result_args = match result_type.as_ref() {
        Type::Path(path) if path.qself.is_none() => path.path.segments.last().and_then(|segment| {
            if segment.ident != "Result" {
                return None;
            }
            match &segment.arguments {
                PathArguments::AngleBracketed(arguments) if arguments.args.len() == 2 => {
                    Some(&arguments.args)
                }
                _ => None,
            }
        }),
        _ => None,
    };
    let valid_result = result_args.is_some_and(|arguments| matches!(arguments.first(), Some(GenericArgument::Type(_))) && matches!(arguments.last(), Some(GenericArgument::Type(Type::Path(path))) if path.qself.is_none() && path.path.segments.last().is_some_and(|segment| segment.ident == "CommandError" && matches!(segment.arguments, PathArguments::None))));
    if !valid_result {
        return Err(syn::Error::new_spanned(
            result_type,
            "shared commands must return Result<T, CommandError>",
        ));
    }
    validate_owned_type(result_type)?;
    let Pat::Ident(service_name) = service.pat.as_ref() else {
        unreachable!()
    };
    let service_name = &service_name.ident;
    let mut arguments = Vec::new();
    let mut argument_types = Vec::new();
    for argument in signature.inputs.iter().skip(1) {
        let FnArg::Typed(argument) = argument else {
            unreachable!()
        };
        validate_owned_type(&argument.ty)?;
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            unreachable!()
        };
        arguments.push(pattern.ident.clone());
        argument_types.push(argument.ty.clone());
    }
    let name = &signature.ident;
    let method = name.to_string();
    let visibility = &function.vis;
    let attributes = &function.attrs;
    let output = &signature.output;
    let asynchronous = &signature.asyncness;
    let awaited = asynchronous.map(|_| quote!(.await));
    let args_type = fresh_identifier("__WbookRpcArgs", quote!(#function));
    let mut checked_signature = signature.clone();
    checked_signature.ident = format_ident!("call");
    for argument in &mut checked_signature.inputs {
        if let FnArg::Typed(argument) = argument {
            if let Pat::Ident(pattern) = argument.pat.as_mut() {
                pattern.mutability = None;
            }
        }
    }

    let checked = quote! {
        #visibility mod #name {
            use super::*;

            pub #checked_signature {
                crate::commands::dto::check_integers(
                    &(#(&#arguments,)*),
                    crate::commands::dto::ErrorKind::InvalidParams,
                )?;
                let result = super::#name(#service_name, #(#arguments),*) #awaited;
                let result = result?;
                crate::commands::dto::check_integers(
                    &result,
                    crate::commands::dto::ErrorKind::InternalError,
                )?;
                Ok(result)
            }

            pub async fn rpc(
                app: &::wbook_core::Wbook,
                params: ::serde_json::Value,
            ) -> Result<::serde_json::Value, crate::commands::dto::CommandError> {
                #[derive(::serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct #args_type { #(#arguments: #argument_types,)* }

                let args: #args_type = crate::commands::dto::decode_arguments(#method, params)?;
                let _ = &args;
                let result = call(app, #(args.#arguments),*) #awaited?;
                crate::commands::dto::encode_response(#method, &result)
            }
        }
    };

    let wrapper = quote! {
        #(#attributes)*
        #[::tauri::command]
        #[::specta::specta]
        pub(super) #asynchronous fn #name(
            #service_name: ::tauri::State<'_, ::std::sync::Arc<::wbook_core::Wbook>>,
            #(#arguments: #argument_types,)*
        ) #output {
            super::#name::call(&#service_name, #(#arguments),*) #awaited
        }
    };
    Ok((checked, wrapper))
}

fn validate_owned_type(ty: &Type) -> syn::Result<()> {
    struct OwnedType(Option<syn::Error>);
    impl<'ast> Visit<'ast> for OwnedType {
        fn visit_type(&mut self, ty: &'ast Type) {
            if matches!(
                ty,
                Type::Reference(_)
                    | Type::Ptr(_)
                    | Type::ImplTrait(_)
                    | Type::TraitObject(_)
                    | Type::BareFn(_)
                    | Type::Infer(_)
                    | Type::Macro(_)
            ) {
                if self.0.is_none() {
                    self.0 = Some(syn::Error::new_spanned(
                        ty,
                        "wire arguments and results require concrete owned types",
                    ));
                }
            } else {
                syn::visit::visit_type(self, ty);
            }
        }
    }
    let mut visitor = OwnedType(None);
    visitor.visit_type(ty);
    visitor.0.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests;
