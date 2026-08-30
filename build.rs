// build.rs
use shaderc::{Compiler, ShaderKind};
use std::path::Path;

/// Build all the needed shaders.
fn build_shaders(compiler: &Compiler, shader_dir: &Path, out_dir: &String) {
    if shader_dir.is_dir() {
        for entry in std::fs::read_dir(shader_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if let Some(extension) = path.extension() {
                let kind = match extension.to_str().unwrap() {
                    "vert" => ShaderKind::Vertex,
                    "frag" => ShaderKind::Fragment,
                    "comp" => ShaderKind::Compute,
                    _ => continue,
                };

                let shader_source = std::fs::read_to_string(&path).unwrap();
                let file_name = path.file_name().unwrap().to_str().unwrap();

                let artifact = compiler
                    .compile_into_spirv(&shader_source, kind, file_name, "main", None)
                    .unwrap();

                let output_path = Path::new(&out_dir).join(format!("{file_name}.spv"));
                std::fs::write(output_path, artifact.as_binary_u8()).unwrap();
            }
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=shaders/");
    let compiler = Compiler::new().unwrap();
    let shader_dir = Path::new("shaders");
    let out_dir = std::env::var("OUT_DIR").unwrap();

    build_shaders(&compiler, shader_dir, &out_dir);
}
