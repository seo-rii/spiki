use std::collections::HashSet;
use std::fs;

use serde_json::Value;

use crate::model::BackendState;

use super::state::ViewContext;

const WEB_PROFILES: &[&str] = &[
    "javascript",
    "nodejs",
    "typescript",
    "node-ts",
    "react",
    "react-ts",
    "preact",
    "nextjs",
    "remix",
    "gatsby",
    "vue",
    "nuxt",
    "svelte",
    "sveltekit",
    "angular",
    "astro",
    "solid",
    "solidstart",
    "qwik",
    "ember",
    "lit",
    "alpine",
];

const GENERAL_PROFILES: &[&str] = &[
    "c",
    "c-native",
    "cpp",
    "cpp-native",
    "java",
    "java-jvm",
    "java-maven",
    "java-gradle",
    "kotlin",
    "kotlin-jvm",
    "python",
    "pyproject-python",
    "go",
    "go-module",
    "rust",
    "cargo-rust",
    "ruby",
    "swift",
    "swift-package",
    "csharp",
    "dotnet-csharp",
    "fsharp",
    "dotnet-fsharp",
    "vbnet",
    "dotnet-vbnet",
    "scala",
    "scala-sbt",
    "haskell",
    "haskell-cabal",
    "haskell-stack",
    "ocaml",
    "ocaml-opam",
    "pascal",
    "d",
    "php",
    "perl",
    "lua",
    "shell",
    "bash",
    "assembly",
    "objective-c",
    "objective-cpp",
    "fortran",
    "scheme",
    "ada",
    "awk",
    "tcl",
    "r",
    "julia",
    "clojure",
    "common-lisp",
    "erlang",
    "elixir",
    "dart",
    "nim",
    "prolog",
    "freebasic",
    "haxe",
    "systemverilog",
];

pub(crate) fn backend_for_language(language: impl Into<String>) -> BackendState {
    let language = language.into();
    let provider = if WEB_PROFILES.contains(&language.as_str()) {
        format!("phase1-web:{language}")
    } else if GENERAL_PROFILES.contains(&language.as_str()) {
        format!("phase1-general:{language}")
    } else {
        String::from("phase1-skeleton")
    };

    BackendState {
        language,
        state: String::from("off"),
        provider: Some(provider),
        idle_for_ms: Some(0),
        last_error: None,
    }
}

pub(crate) fn detected_backends(view: &ViewContext) -> Vec<BackendState> {
    let (files, cached_backends) = {
        let meta = view.workspace.meta.lock();
        (
            meta.known_files.keys().cloned().collect::<Vec<_>>(),
            meta.semantic_backends.clone(),
        )
    };
    let mut dependencies = HashSet::new();
    let mut extensions = HashSet::new();
    let mut file_names = HashSet::new();

    for path in &files {
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            let name = name.to_ascii_lowercase();
            if name == "package.json" {
                if let Ok(text) = fs::read_to_string(path) {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        for field in [
                            "dependencies",
                            "devDependencies",
                            "peerDependencies",
                            "optionalDependencies",
                        ] {
                            let Some(object) = value.get(field).and_then(Value::as_object) else {
                                continue;
                            };
                            dependencies.extend(object.keys().cloned());
                        }
                    }
                }
            }
            file_names.insert(name);
        }

        if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            extensions.insert(extension.to_ascii_lowercase());
        }
    }

    let has_js_source = ["js", "mjs", "cjs"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_ts_source = ["ts", "mts", "cts"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_jsx = extensions.contains("jsx");
    let has_tsx = extensions.contains("tsx");
    let has_vue_file = extensions.contains("vue");
    let has_svelte_file = extensions.contains("svelte");
    let has_astro_file = extensions.contains("astro");
    let has_c_source = extensions.contains("c");
    let has_cpp_source = ["cc", "cpp", "cxx", "c++", "ixx", "hpp", "hxx"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_java_source = extensions.contains("java");
    let has_kotlin_source = ["kt", "kts"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_python_source = extensions.contains("py");
    let has_go_source = extensions.contains("go");
    let has_rust_source = extensions.contains("rs");
    let has_ruby_source = extensions.contains("rb");
    let has_swift_source = extensions.contains("swift");
    let has_csharp_source = extensions.contains("cs");
    let has_fsharp_source = ["fs", "fsi", "fsx"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_vbnet_source = extensions.contains("vb");
    let has_scala_source = ["scala", "sc"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_haskell_source = ["hs", "lhs"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_ocaml_source = ["ml", "mli"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_pascal_source = ["pas", "pp", "lpr"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_d_source = extensions.contains("d");
    let has_php_source = ["php", "phtml"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_perl_source = ["pl", "pm"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_lua_source = extensions.contains("lua");
    let has_shell_source = ["sh", "bash", "zsh"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_assembly_source = ["asm", "nasm", "s"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_objective_cpp_source = extensions.contains("mm");
    let has_fortran_source = ["f", "for", "f90", "f95", "f03", "f08"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_scheme_source = ["scm", "ss"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_ada_source = ["adb", "ads"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_awk_source = extensions.contains("awk");
    let has_tcl_source = extensions.contains("tcl");
    let has_r_source = extensions.contains("r");
    let has_julia_source = extensions.contains("jl");
    let has_clojure_source = ["clj", "cljs", "cljc"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_common_lisp_source = ["lisp", "lsp", "cl"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_erlang_source = ["erl", "hrl"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_elixir_source = ["ex", "exs"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_dart_source = extensions.contains("dart");
    let has_nim_source = extensions.contains("nim");
    let has_prolog_source = ["pro", "prolog"]
        .iter()
        .any(|extension| extensions.contains(*extension));
    let has_freebasic_source = extensions.contains("bas");
    let has_haxe_source = extensions.contains("hx");
    let has_systemverilog_source = ["sv", "svh"]
        .iter()
        .any(|extension| extensions.contains(*extension));

    let has_package_json = file_names.contains("package.json");
    let has_tsconfig = [
        "tsconfig.json",
        "tsconfig.app.json",
        "tsconfig.base.json",
        "tsconfig.node.json",
        "jsconfig.json",
    ]
    .iter()
    .any(|file_name| file_names.contains(*file_name));
    let has_native_build = [
        "cmakelists.txt",
        "makefile",
        "meson.build",
        "compile_commands.json",
    ]
    .iter()
    .any(|file_name| file_names.contains(*file_name));
    let has_pyproject = [
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "requirements.txt",
        "pipfile",
        "tox.ini",
    ]
    .iter()
    .any(|file_name| file_names.contains(*file_name));
    let has_go_mod = file_names.contains("go.mod");
    let has_cargo = file_names.contains("cargo.toml");
    let has_java_maven = ["pom.xml", "mvnw"]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_java_gradle = [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
        "gradle.properties",
    ]
    .iter()
    .any(|file_name| file_names.contains(*file_name));
    let has_scala_sbt = file_names.contains("build.sbt");
    let has_swift_package = file_names.contains("package.swift");
    let has_haskell_cabal = extensions.contains("cabal") || file_names.contains("cabal.project");
    let has_haskell_stack = file_names.contains("stack.yaml");
    let has_ocaml_opam = file_names.contains("dune-project") || extensions.contains("opam");
    let has_dotnet = ["csproj", "fsproj", "vbproj", "sln"]
        .iter()
        .any(|extension| extensions.contains(*extension))
        || ["global.json", "nuget.config"]
            .iter()
            .any(|file_name| file_names.contains(*file_name));
    let has_objective_c_source = extensions.contains("m") && has_native_build;

    let has_react = dependencies.contains("react") || dependencies.contains("react-dom");
    let has_preact = dependencies.contains("preact");
    let has_nextjs = dependencies.contains("next")
        || [
            "next.config.js",
            "next.config.mjs",
            "next.config.cjs",
            "next.config.ts",
        ]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_remix = dependencies.contains("@remix-run/react")
        || dependencies.contains("@remix-run/dev")
        || [
            "remix.config.js",
            "remix.config.mjs",
            "remix.config.cjs",
            "remix.config.ts",
        ]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_gatsby = dependencies.contains("gatsby")
        || [
            "gatsby-config.js",
            "gatsby-config.mjs",
            "gatsby-config.cjs",
            "gatsby-config.ts",
        ]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_vue = dependencies.contains("vue") || has_vue_file;
    let has_nuxt = dependencies.contains("nuxt")
        || [
            "nuxt.config.js",
            "nuxt.config.mjs",
            "nuxt.config.cjs",
            "nuxt.config.ts",
        ]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_svelte = dependencies.contains("svelte") || has_svelte_file;
    let has_sveltekit = dependencies.contains("@sveltejs/kit");
    let has_angular = dependencies.contains("@angular/core") || file_names.contains("angular.json");
    let has_astro = dependencies.contains("astro")
        || has_astro_file
        || [
            "astro.config.js",
            "astro.config.mjs",
            "astro.config.cjs",
            "astro.config.ts",
        ]
        .iter()
        .any(|file_name| file_names.contains(*file_name));
    let has_solid = dependencies.contains("solid-js");
    let has_solidstart = dependencies.contains("@solidjs/start");
    let has_qwik =
        dependencies.contains("@builder.io/qwik") || dependencies.contains("@builder.io/qwik-city");
    let has_ember = dependencies.contains("ember-source") || dependencies.contains("ember-cli");
    let has_lit = dependencies.contains("lit")
        || dependencies.contains("lit-html")
        || dependencies.contains("lit-element");
    let has_alpine = dependencies.contains("alpinejs");
    let has_typescript = has_ts_source || has_tsx || has_tsconfig || has_angular;
    let has_javascript = has_js_source
        || has_jsx
        || has_vue_file
        || has_svelte_file
        || has_astro_file
        || has_ember
        || has_lit
        || has_alpine;
    let has_java_project = has_java_source || has_java_maven || has_java_gradle;
    let has_python_project = has_python_source || has_pyproject;
    let has_go_project = has_go_source || has_go_mod;
    let has_rust_project = has_rust_source || has_cargo;
    let has_swift_project = has_swift_source || has_swift_package;
    let has_scala_project = has_scala_source || has_scala_sbt;
    let has_haskell_project = has_haskell_source || has_haskell_cabal || has_haskell_stack;
    let has_ocaml_project = has_ocaml_source || has_ocaml_opam;

    let mut languages = Vec::new();
    let mut seen = HashSet::new();
    let mut add_language = |language: &'static str| {
        if seen.insert(language) {
            languages.push(language);
        }
    };

    if has_typescript {
        add_language("typescript");
        if has_package_json {
            add_language("node-ts");
        }
    }
    if has_javascript {
        add_language("javascript");
        if has_package_json {
            add_language("nodejs");
        }
    }
    if has_react || has_nextjs || has_remix || has_gatsby {
        add_language(if has_typescript { "react-ts" } else { "react" });
    }
    if has_preact {
        add_language("preact");
    }
    if has_nextjs {
        add_language("nextjs");
    }
    if has_remix {
        add_language("remix");
    }
    if has_gatsby {
        add_language("gatsby");
    }
    if has_vue || has_nuxt {
        add_language("vue");
    }
    if has_nuxt {
        add_language("nuxt");
    }
    if has_svelte || has_sveltekit {
        add_language("svelte");
    }
    if has_sveltekit {
        add_language("sveltekit");
    }
    if has_angular {
        add_language("angular");
    }
    if has_astro {
        add_language("astro");
    }
    if has_solid || has_solidstart {
        add_language("solid");
    }
    if has_solidstart {
        add_language("solidstart");
    }
    if has_qwik {
        add_language("qwik");
    }
    if has_ember {
        add_language("ember");
    }
    if has_lit {
        add_language("lit");
    }
    if has_alpine {
        add_language("alpine");
    }

    if has_c_source {
        add_language("c");
        if has_native_build {
            add_language("c-native");
        }
    }
    if has_cpp_source {
        add_language("cpp");
        if has_native_build {
            add_language("cpp-native");
        }
    }
    if has_java_project {
        add_language("java");
        add_language("java-jvm");
    }
    if has_java_maven {
        add_language("java-maven");
    }
    if has_java_gradle {
        add_language("java-gradle");
    }
    if has_kotlin_source {
        add_language("kotlin");
        add_language("kotlin-jvm");
    }
    if has_python_project {
        add_language("python");
        if has_pyproject {
            add_language("pyproject-python");
        }
    }
    if has_go_project {
        add_language("go");
        if has_go_mod {
            add_language("go-module");
        }
    }
    if has_rust_project {
        add_language("rust");
        if has_cargo {
            add_language("cargo-rust");
        }
    }
    if has_ruby_source {
        add_language("ruby");
    }
    if has_swift_project {
        add_language("swift");
        if has_swift_package {
            add_language("swift-package");
        }
    }
    if has_csharp_source || extensions.contains("csproj") {
        add_language("csharp");
        if has_dotnet {
            add_language("dotnet-csharp");
        }
    }
    if has_fsharp_source || extensions.contains("fsproj") {
        add_language("fsharp");
        if has_dotnet {
            add_language("dotnet-fsharp");
        }
    }
    if has_vbnet_source || extensions.contains("vbproj") {
        add_language("vbnet");
        if has_dotnet {
            add_language("dotnet-vbnet");
        }
    }
    if has_scala_project {
        add_language("scala");
        if has_scala_sbt {
            add_language("scala-sbt");
        }
    }
    if has_haskell_project {
        add_language("haskell");
        if has_haskell_cabal {
            add_language("haskell-cabal");
        }
        if has_haskell_stack {
            add_language("haskell-stack");
        }
    }
    if has_ocaml_project {
        add_language("ocaml");
        if has_ocaml_opam {
            add_language("ocaml-opam");
        }
    }
    if has_pascal_source {
        add_language("pascal");
    }
    if has_d_source {
        add_language("d");
    }
    if has_php_source {
        add_language("php");
    }
    if has_perl_source {
        add_language("perl");
    }
    if has_lua_source {
        add_language("lua");
    }
    if has_shell_source {
        add_language("shell");
        add_language("bash");
    }
    if has_assembly_source {
        add_language("assembly");
    }
    if has_objective_c_source {
        add_language("objective-c");
    }
    if has_objective_cpp_source {
        add_language("objective-cpp");
    }
    if has_fortran_source {
        add_language("fortran");
    }
    if has_scheme_source {
        add_language("scheme");
    }
    if has_ada_source {
        add_language("ada");
    }
    if has_awk_source {
        add_language("awk");
    }
    if has_tcl_source {
        add_language("tcl");
    }
    if has_r_source {
        add_language("r");
    }
    if has_julia_source {
        add_language("julia");
    }
    if has_clojure_source {
        add_language("clojure");
    }
    if has_common_lisp_source {
        add_language("common-lisp");
    }
    if has_erlang_source {
        add_language("erlang");
    }
    if has_elixir_source {
        add_language("elixir");
    }
    if has_dart_source {
        add_language("dart");
    }
    if has_nim_source {
        add_language("nim");
    }
    if has_prolog_source {
        add_language("prolog");
    }
    if has_freebasic_source {
        add_language("freebasic");
    }
    if has_haxe_source {
        add_language("haxe");
    }
    if has_systemverilog_source {
        add_language("systemverilog");
    }

    drop(add_language);
    if languages.is_empty() {
        languages.push("typescript");
        languages.push("javascript");
    }

    let mut leaves = Vec::new();
    for language in &languages {
        let mut is_leaf = true;

        for other in &languages {
            if language == other {
                continue;
            }

            let mut parent = match *other {
                "nodejs" => Some("javascript"),
                "react" => Some("nodejs"),
                "nextjs" | "remix" | "gatsby" => Some("react"),
                "preact" | "vue" | "svelte" | "angular" | "astro" | "solid" | "qwik" | "lit"
                | "ember" | "alpine" => Some("nodejs"),
                "nuxt" => Some("vue"),
                "sveltekit" => Some("svelte"),
                "solidstart" => Some("solid"),
                "node-ts" => Some("typescript"),
                "react-ts" => Some("node-ts"),
                "c-native" => Some("c"),
                "cpp-native" => Some("cpp"),
                "java-jvm" | "java-maven" | "java-gradle" => Some("java"),
                "kotlin-jvm" => Some("kotlin"),
                "pyproject-python" => Some("python"),
                "go-module" => Some("go"),
                "cargo-rust" => Some("rust"),
                "swift-package" => Some("swift"),
                "dotnet-csharp" => Some("csharp"),
                "dotnet-fsharp" => Some("fsharp"),
                "dotnet-vbnet" => Some("vbnet"),
                "scala-sbt" => Some("scala"),
                "haskell-cabal" | "haskell-stack" => Some("haskell"),
                "ocaml-opam" => Some("ocaml"),
                "bash" => Some("shell"),
                _ => None,
            };

            while let Some(value) = parent {
                if value == *language {
                    is_leaf = false;
                    break;
                }

                parent = match value {
                    "react" => Some("nodejs"),
                    "vue" | "svelte" | "solid" => Some("nodejs"),
                    "nodejs" => Some("javascript"),
                    "react-ts" => Some("node-ts"),
                    "node-ts" => Some("typescript"),
                    _ => None,
                };
            }

            if !is_leaf {
                break;
            }
        }

        if is_leaf {
            leaves.push(*language);
        }
    }

    leaves
        .into_iter()
        .map(|language| {
            cached_backends
                .get(language)
                .cloned()
                .unwrap_or_else(|| backend_for_language(language))
        })
        .collect()
}
