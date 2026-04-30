// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Build script for edit-wasm.
//!
//! Generates `i18n_edit.rs` from `i18n/edit.toml` (same as the main `edit`
//! crate build script) so that the `localization` module included via
//! `#[path]` can find the generated file in this crate's `OUT_DIR`.

use std::env::VarError;
use std::fmt::Write as _;

fn env_opt(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) => value,
        Err(VarError::NotPresent) => String::new(),
        Err(VarError::NotUnicode(_)) => {
            panic!("Environment variable `{name}` is not valid Unicode")
        }
    }
}

fn main() {
    // The ICU SONAME env-vars are read by `crates/edit/src/sys/unix.rs` at
    // compile time.  For the WASM target that sys module is never compiled, but
    // the build script of the `edit` library will still set them.  We do NOT
    // need to set them here; this build script only generates `i18n_edit.rs`.
    compile_i18n();

    // Tell Cargo when to re-run this script.
    println!("cargo::rerun-if-changed=../../i18n/edit.toml");
    println!("cargo::rerun-if-env-changed=EDIT_CFG_LANGUAGES");
}

fn compile_i18n() {
    let i18n_path = "../../i18n/edit.toml";
    let i18n = std::fs::read_to_string(i18n_path)
        .unwrap_or_else(|e| panic!("Failed to read {i18n_path}: {e}"));
    let contents = generate_i18n(&i18n);
    let out_dir = env_opt("OUT_DIR");
    let path = format!("{out_dir}/i18n_edit.rs");
    std::fs::write(&path, contents).unwrap();
}

// ── i18n code generator ───────────────────────────────────────────────────────
//
// This is a copy of the logic from `crates/edit/build/i18n.rs`, reproduced
// here so that `edit-wasm` has no build-script dependency on `edit`'s build
// internals.

use std::collections::{BTreeMap, HashMap, HashSet};

fn generate_i18n(definitions: &str) -> String {
    let i18n = toml_span::parse(definitions).expect("Failed to parse i18n file");
    let root = i18n.as_table().unwrap();
    let mut languages = Vec::new();
    let mut aliases = Vec::new();
    let mut translations: BTreeMap<String, HashMap<String, String>> = BTreeMap::new();

    for (k, v) in root.iter() {
        match &k.name[..] {
            "__default__" => {
                const ERROR: &str = "i18n: __default__ must be [str]";
                languages = Vec::from_iter(
                    v.as_array()
                        .expect(ERROR)
                        .iter()
                        .map(|lang| lang.as_str().expect(ERROR).to_string()),
                );
            }
            "__alias__" => {
                const ERROR: &str = "i18n: __alias__ must be str->str";
                aliases.extend(v.as_table().expect(ERROR).iter().map(|(alias, lang)| {
                    (alias.to_string(), lang.as_str().expect(ERROR).to_string())
                }));
            }
            _ => {
                const ERROR: &str = "i18n: LocId must be str->str";
                translations.insert(
                    k.name.to_string(),
                    HashMap::from_iter(
                        v.as_table().expect(ERROR).iter().map(|(k, v)| {
                            (k.name.to_string(), v.as_str().expect(ERROR).to_string())
                        }),
                    ),
                );
            }
        }
    }

    let cfg_languages = env_opt("EDIT_CFG_LANGUAGES");
    if !cfg_languages.is_empty() {
        languages = cfg_languages.split(',').map(|lang| lang.to_string()).collect();
    }

    if !languages.iter().any(|l| l == "en") {
        languages.push("en".to_string());
    }

    for lang in &mut languages {
        if lang.is_empty() {
            panic!("i18n: empty language tag");
        }
        for c in unsafe { lang.as_bytes_mut() } {
            *c = match *c {
                b'A'..=b'Z' | b'a'..=b'z' | b'-' => c.to_ascii_lowercase(),
                _ => panic!("i18n: language tag \"{lang}\" must be [a-zA-Z-]"),
            }
        }
    }

    let mut languages_with_aliases: Vec<_>;
    {
        let mut specified = HashSet::new();
        for lang in &languages {
            if !specified.insert(lang.as_str()) {
                panic!("i18n: duplicate language tag \"{lang}\"");
            }
        }

        let mut available = HashSet::new();
        for v in translations.values() {
            for lang in v.keys() {
                available.insert(lang.as_str());
            }
        }

        let mut invalid = Vec::new();
        for lang in &languages {
            if !available.contains(lang.as_str()) {
                invalid.push(lang.as_str());
            }
        }
        if !invalid.is_empty() {
            panic!("i18n: invalid language tags {invalid:?}");
        }

        languages_with_aliases = languages.iter().map(|l| (l.clone(), l.clone())).collect();
        for (alias, lang) in aliases {
            if specified.contains(lang.as_str()) && !specified.contains(alias.as_str()) {
                languages_with_aliases.push((alias, lang));
            }
        }
    }

    {
        fn sort(a: &String, b: &String) -> std::cmp::Ordering {
            match (a == "en", b == "en") {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let (a0, a1) = a.split_once('-').unwrap_or((a, "xxxxxx"));
                    let (b0, b1) = b.split_once('-').unwrap_or((b, "xxxxxx"));
                    match a0.cmp(b0) {
                        std::cmp::Ordering::Equal => a1.cmp(b1),
                        ord => ord,
                    }
                }
            }
        }
        languages.sort_unstable_by(sort);
        languages_with_aliases.sort_unstable_by(|a, b| sort(&a.0, &b.0));
    }

    let mut out = String::new();

    _ = write!(
        out,
        "\
// This file is generated by build.rs. Do not edit it manually.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LocId {{",
    );

    for (k, _) in translations.iter() {
        _ = writeln!(out, "    {k},");
    }

    _ = write!(
        out,
        "\
}}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LangId {{
",
    );

    for lang in &languages {
        _ = writeln!(out, "    {},", hyphen_to_underscore(lang));
    }

    _ = write!(
        out,
        "\
}}

const LANGUAGES: &[(&str, LangId)] = &[
"
    );

    for (alias, lang) in &languages_with_aliases {
        _ = writeln!(out, "    ({alias:?}, LangId::{}),", hyphen_to_underscore(lang));
    }

    _ = write!(
        out,
        "\
];

const TRANSLATIONS: [[&str; {}]; {}] = [
",
        translations.len(),
        languages.len(),
    );

    for lang in &languages {
        _ = writeln!(out, "    [");
        for (_, v) in translations.iter() {
            const DEFAULT: &String = &String::new();
            let v = v.get(lang).or_else(|| v.get("en")).unwrap_or(DEFAULT);
            _ = writeln!(out, "        {v:?},");
        }
        _ = writeln!(out, "    ],");
    }

    _ = writeln!(out, "];");

    out
}

fn hyphen_to_underscore(s: &str) -> String {
    s.chars().map(|c| if c == '-' { '_' } else { c }).collect()
}
