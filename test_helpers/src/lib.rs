use proc_macro::TokenStream;
use quote::quote;

#[proc_macro]
pub fn include_class(input: TokenStream) -> TokenStream {
    let class_name = syn::parse_macro_input!(input as syn::LitStr).value();
    let file_path = format!("{}{class_name}.class", env!("JAVA_OUT_DIR"));
    quote! {
        include_bytes!(#file_path)
    }.into()
}

#[proc_macro]
pub fn java_version(_: TokenStream) -> TokenStream {
    let java_version = env!("JAVA_VERSION");
    quote! {
        #java_version
    }.into()
}
