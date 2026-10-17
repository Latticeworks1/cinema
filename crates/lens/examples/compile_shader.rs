//! Compile lens.sksl with Skia and print the compiler's message on failure.
use fframes_skia_renderer::skia_safe::RuntimeEffect;

fn main() {
    match RuntimeEffect::make_for_shader(include_str!("../shaders/lens.sksl"), None) {
        Ok(_) => println!("lens.sksl compiles"),
        Err(e) => {
            eprintln!("lens.sksl does not compile:\n{e}");
            std::process::exit(1);
        }
    }
}
