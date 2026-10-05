use super::*;

fn expanded(source: &str) -> ItemMod {
    syn::parse2(expand(syn::parse_str(source).unwrap()).unwrap()).unwrap()
}

fn items(module: &ItemMod) -> &[Item] {
    &module.content.as_ref().unwrap().1
}

fn function<'a>(module: &'a ItemMod, name: &str) -> &'a ItemFn {
    items(module)
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) if function.sig.ident == name => Some(function),
            _ => None,
        })
        .unwrap()
}

fn nested<'a>(module: &'a ItemMod, name: &str) -> &'a ItemMod {
    items(module)
        .iter()
        .find_map(|item| match item {
            Item::Mod(module) if module.ident == name => Some(module),
            _ => None,
        })
        .unwrap()
}

#[test]
fn shared_and_desktop_declarations_generate_one_registry() {
    let module = expanded(
        r#"
        pub mod commands {
            use super::*;
            #[desktop_only]
            #[query]
            pub fn get_port(port: tauri::State<'_, Port>) -> u16 { port.0 }
            pub async fn read(app: &Wbook, session_id: SessionId) -> Result<String, CommandError> {
                app.read(session_id).await
            }
        }
    "#,
    );
    let wrappers = nested(&module, "__wbook_tauri_commands");
    let wrapper = function(wrappers, "read");
    assert!(wrapper.sig.asyncness.is_some());
    assert_eq!(wrapper.attrs.len(), 2);
    let signature = &wrapper.sig;
    assert!(quote!(#signature).to_string().contains("tauri :: State"));
    let call = function(nested(&module, "read"), "call");
    let body = &call.block;
    let body = quote!(#body).to_string();
    assert!(body.find("InvalidParams").unwrap() < body.find("super :: read").unwrap());
    assert!(body.find("InternalError").unwrap() > body.find("super :: read").unwrap());
    let rpc = function(nested(&module, "read"), "rpc");
    let body = &rpc.block;
    let body = quote!(#body).to_string();
    assert!(body.contains("rename_all = \"camelCase\""));
    assert!(body.contains("decode_arguments (\"read\" , params)"));
    assert!(body.contains("encode_response (\"read\" , & result)"));
    assert!(body.contains("call (app , args . session_id) . await"));
    let builder = function(&module, "builder");
    let body = &builder.block;
    let body = quote!(#body).to_string();
    assert!(body.contains(
        "CommandSet :: new (:: tauri_specta :: collect_commands ! [__wbook_tauri_commands :: get_port] , :: tauri_specta :: collect_commands ! [__wbook_tauri_commands :: read] ,)"
    ));
    assert!(body.contains("ErrorHandlingMode :: Throw"));
    let dispatch = function(&module, "dispatch");
    let body = &dispatch.block;
    let body = quote!(#body).to_string();
    assert!(body.contains("\"read\" => read :: rpc"));
    assert!(!body.contains("get_port :: rpc"));
    assert!(body.contains("PlatformUnsupported"));
    assert_eq!(function(wrappers, "get_port").attrs.len(), 2);
}

#[test]
fn synchronous_zero_input_commands_keep_sync_wrappers() {
    let module = expanded(
        r#"
        mod commands {
            pub fn list(app: &Wbook) -> Result<Vec<SessionSnapshot>, CommandError> {
                Ok(app.list())
            }
        }
    "#,
    );
    assert!(function(&module, "list").sig.asyncness.is_none());
    assert!(function(nested(&module, "__wbook_tauri_commands"), "list")
        .sig
        .asyncness
        .is_none());
    assert!(function(nested(&module, "list"), "call")
        .sig
        .asyncness
        .is_none());
    let rpc = function(nested(&module, "list"), "rpc");
    assert!(rpc.sig.asyncness.is_some());
    let body = &rpc.block;
    let body = quote!(#body).to_string();
    assert!(body.contains("struct __WbookRpcArgs { }"));
    assert!(!body.contains(". await"));
    assert!(body.contains("call (app ,)"));
}

#[test]
fn original_functions_keep_their_scope_and_attributes() {
    let source: ItemMod = syn::parse_quote! {
        mod commands {
            use super::*;
            #[allow(unused_variables)]
            pub fn first(app: &Wbook, mut result: u32) -> Result<u32, CommandError> {
                result += 1;
                self::second(app, result)
            }
            pub fn second(app: &Wbook, value: u32) -> Result<u32, CommandError> {
                if value == 0 { Ok(0) } else { second(app, value - 1) }
            }
        }
    };
    let module: ItemMod = syn::parse2(expand(source.clone()).unwrap()).unwrap();
    for name in ["first", "second"] {
        let original = function(&source, name);
        let retained = function(&module, name);
        assert_eq!(quote!(#original).to_string(), quote!(#retained).to_string());
        let checked = function(nested(&module, name), "call");
        let signature = &checked.sig;
        assert!(!quote!(#signature).to_string().contains("mut result"));
    }
}

#[test]
fn generated_names_do_not_shadow_input_types_or_arguments() {
    let module = expanded(
        r#"
        mod commands {
            use super::*;
            pub fn __wbook_tauri_commands(
                context: &Wbook,
                args: Args,
                params: __WbookRpcArgs,
                result: String,
                app: u32,
            ) -> Result<Args, CommandError> {
                Ok(args)
            }
        }
    "#,
    );
    let wrappers = nested(&module, "__wbook_tauri_commands_");
    assert_eq!(
        function(wrappers, "__wbook_tauri_commands")
            .sig
            .inputs
            .len(),
        5
    );
    let rpc = function(nested(&module, "__wbook_tauri_commands"), "rpc");
    let body = &rpc.block;
    let body = quote!(#body).to_string();
    assert!(body.contains("struct __WbookRpcArgs_ { args : Args , params : __WbookRpcArgs , result : String , app : u32 , }"), "{body}");
    assert!(body.contains("call (app , args . args , args . params , args . result , args . app)"));
}

#[test]
fn unsupported_signatures_produce_targeted_diagnostics() {
    for (function, message) in [
        (
            "fn read() -> Result<(), CommandError> { Ok(()) }",
            "initial &Wbook",
        ),
        (
            "fn read(app: &mut Wbook) -> Result<(), CommandError> { Ok(()) }",
            "immutable &Wbook",
        ),
        (
            "fn read(app: &Wbook, text: &str) -> Result<(), CommandError> { Ok(()) }",
            "concrete owned types",
        ),
        (
            "fn read(app: &Wbook, text: Option<&str>) -> Result<(), CommandError> { Ok(()) }",
            "concrete owned types",
        ),
        (
            "fn read<T>(app: &Wbook, text: T) -> Result<(), CommandError> { Ok(()) }",
            "generic commands",
        ),
        (
            "fn read(&self) -> Result<(), CommandError> { Ok(()) }",
            "with self",
        ),
        (
            "unsafe fn read(app: &Wbook) -> Result<(), CommandError> { Ok(()) }",
            "unsafe",
        ),
        (
            "extern \"C\" fn read(app: &Wbook) -> Result<(), CommandError> { Ok(()) }",
            "extern",
        ),
        (
            "fn read(app: &Wbook, (x, y): (u8, u8)) -> Result<(), CommandError> { Ok(()) }",
            "identifier patterns",
        ),
        (
            "fn read(app: &Wbook) -> String { String::new() }",
            "Result<T, CommandError>",
        ),
        (
            "fn read(app: &Wbook) -> Result<(), String> { Ok(()) }",
            "Result<T, CommandError>",
        ),
        (
            "#[cfg(test)] fn read(app: &Wbook) -> Result<(), CommandError> { Ok(()) }",
            "conditional command",
        ),
        (
            "fn builder(app: &Wbook) -> Result<(), CommandError> { Ok(()) }",
            "reserved",
        ),
        ("#[desktop_only(extra)] fn read() {}", "without arguments"),
        ("#[query] #[query] fn read() {}", "use query once"),
    ] {
        let module = syn::parse_str(&format!("mod commands {{ {function} }}")).unwrap();
        let error = expand(module).unwrap_err();
        assert!(error.to_string().contains(message), "{function}: {error}");
        assert!(!error.into_compile_error().is_empty());
    }
}

#[test]
fn unsupported_modules_and_duplicate_commands_fail() {
    for (source, message) in [
        ("mod commands;", "inline module"),
        ("mod commands {}", "at least one command"),
        (
            "mod commands { const EXTRA: u16 = 1; }",
            "only use declarations",
        ),
        (
            "mod commands { #[desktop_only] fn read() {} #[desktop_only] fn read() {} }",
            "duplicate command",
        ),
    ] {
        let error = expand(syn::parse_str(source).unwrap()).unwrap_err();
        assert!(error.to_string().contains(message), "{source}: {error}");
    }
}
