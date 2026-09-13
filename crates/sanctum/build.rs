use std::{collections::HashMap, path::PathBuf};

fn main() {
    let libraries = HashMap::from([("lucide".to_owned(), PathBuf::from(lucide_slint::lib()))]);
    let config = slint_build::CompilerConfiguration::new().with_library_paths(libraries);
    slint_build::compile_with_config("ui/sanctum.slint", config)
        .expect("failed to compile Sanctum UI");
}
