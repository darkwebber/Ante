//! Names the prelude and the standard library modules export (spec §8.5,
//! §13.5, §21). Until the standard library ships as "Ante" source (M10),
//! these tables stand in for it during name resolution.

/// The module holding the standard library written in "Ante" itself
/// (`antec/std/ante/`). Its name can't be written in source, so programs
/// reach its public items only through the prelude fallback.
pub const ANTE_SOURCE_MODULE: &str = "<ante>";

/// The "Ante" source of that module, compiled together with every program.
pub const ANTE_SOURCE: &str = include_str!("../std/ante/prelude.ante");

/// The file name its diagnostics and traps point at.
pub const ANTE_SOURCE_FILE: &str = "<ante>/prelude.ante";

/// Standard modules with a part written in "Ante" (`antec/std/ante/<name>.ante`).
/// A program compiles one only when it (or another such part) imports the
/// module. The part is its own hidden module, `<ante>/<name>`; its public
/// items are reached through the standard module's name, next to the
/// intrinsics the module's signature file declares.
pub const ANTE_MODULES: &[(&str, &str)] = &[
    ("prelude/data", include_str!("../std/ante/data.ante")),
    ("encoding", include_str!("../std/ante/encoding.ante")),
    ("json", include_str!("../std/ante/json.ante")),
    ("form", include_str!("../std/ante/form.ante")),
    ("query", include_str!("../std/ante/query.ante")),
    ("samayam", include_str!("../std/ante/samayam.ante")),
    ("bhadram", include_str!("../std/ante/bhadram.ante")),
    ("log", include_str!("../std/ante/log.ante")),
    ("config", include_str!("../std/ante/config.ante")),
    ("random", include_str!("../std/ante/random.ante")),
];

/// Parts of the prelude written in "Ante" (`prelude/<part>` above) that a
/// program compiles only when its text mentions one of their words, so small
/// programs stay quick to build. Their public items are in the prelude.
pub const PRELUDE_PART_WORDS: &[(&str, &[&str])] = &[(
    "prelude/data",
    &[
        "Raayochu",
        "Chadhavochu",
        "Kotthadhi",
        "Rahasyam",
        "DataChettu",
        "Raasevaadu",
        "Chadhivevaadu",
        "ChadhuvuThappu",
        "JodiChadhuvu",
    ],
)];

/// Whether a source file is part of the standard library written in "Ante"
/// (the prelude, its parts, and the modules' parts) rather than the program.
pub fn is_ante_source_file(name: &str) -> bool {
    name.starts_with("<ante>/")
}

/// Whether a hidden module is a part of the prelude.
pub fn is_prelude_part(module: &str) -> bool {
    module.starts_with("<ante>/prelude/")
}

/// The hidden module holding a standard module's part written in "Ante".
pub fn ante_module_name(module: &str) -> String {
    format!("<ante>/{module}")
}

/// That part's source, if the module has one.
pub fn ante_module_source(module: &str) -> Option<&'static str> {
    ANTE_MODULES.iter().find(|(m, _)| *m == module).map(|(_, s)| *s)
}

/// Built-in type words. The parser keeps them as single-segment type paths.
pub const BUILTIN_TYPES: &[&str] = &[
    "ankam",
    "ankam8",
    "ankam16",
    "ankam32",
    "ankam64",
    "amsam",
    "amsam32",
    "amsam64",
    "ato_ito",
    "padham",
    "aksharam",
    "jatha",
    "jodi",
    "undochu",
    "varusa",
    "mukka",
    "sare_thappu",
    "pette",
    "panchukunna",
    "khali",
    "raadhu",
    "baitlu",
];

pub fn is_builtin_type(name: &str) -> bool {
    BUILTIN_TYPES.contains(&name) || name.starts_with("aasajeevi ")
}

/// Prelude functions: §21.1 plus the iteration adapters of §21.4.2.
pub const PRELUDE_VALUES: &[&str] = &[
    "choopinchu",
    "cheppu",
    "gola_pettu",
    "nijame",
    "kompa_munigindhi",
    "maarchu",
    "kudhirithe_maarchu",
    "kolpothu_maarchu",
    "marpidi",
    "size_of",
    "rakam_peru",
    "align_of",
    "kalipi",
    "modhati",
    "dhaatu",
    "prathi_dhaaniki",
    "jalleda_pattu",
    "poguchey",
    "number_vesi",
    "varusa_ga",
    "anni_theesuko",
];

/// Prelude types that aren't keywords. Their variants (if any) aren't
/// tracked yet, so any `Type.Variant` path is accepted.
pub const PRELUDE_TYPES: &[&str] =
    &["Hasher", "Raatha", "gumpu", "gajibiji_jodi", "Paridhi", "ParseThappu", "Utf8Thappu"];

/// `edho_okati Kramam ante: Thakkuva, Samaanam, Ekkuva` (§13.5).
pub const KRAMAM_VARIANTS: &[&str] = &["Thakkuva", "Samaanam", "Ekkuva"];

/// The standard traits of §13.5.
pub const PRELUDE_TRAITS: &[&str] = &[
    "CopyKottochu",
    "Pampochu",
    "Panchukovochu",
    "XeroxTheeyochu",
    "Sardhukochu",
    "Polchochu",
    "VarusaPettochu",
    "HashCheyochu",
    "Choopinchochu",
    "Okkokkati",
    "Kalupochu",
    "Theeseyochu",
    "Guninchochu",
    "Panchochu",
    "Migulochu",
    "Thippochu",
    "MatmulCheyochu",
    "NunchiRavochu",
];

/// Annotations the compiler knows (§13.6, §18.3, §22, §23.3).
pub const ANNOTATIONS: &[&str] = &["pariksha", "twaraga", "automatic", "bayata", "C", "theliyanivi_vadhiley"];

/// Renamed words (old, new) that still work with warning W0100 for one
/// minor release (§23.5, docs/VERSIONING.md). Spec r7 replaced these
/// with the words people actually say.
pub const RENAMED: &[(&str, &str)] = &[
    ("vegam", "twaraga"),
    ("byte_bhaagam", "byte_mukka"),
    ("mugimpu", "chaalu"),
    ("Sheshamochu", "Migulochu"),
    ("shesham", "migulu"),
];

/// The new spelling of a renamed word.
pub fn renamed(old: &str) -> Option<&'static str> {
    RENAMED.iter().find(|(o, _)| *o == old).map(|(_, n)| *n)
}

/// W0100's message.
pub fn renamed_message(old: &str, new: &str) -> String {
    format!("`{old}` peru maarindhi, ippudu `{new}` ani raayandi (`{old}` next release lo pothundhi)")
}

/// Item annotations the spec defines that `antec` doesn't implement yet
/// (E1202): Part IX and Annex W.
pub const PLANNED_ANNOTATIONS: &[&str] = &["server"];

/// Field and variant annotations of the data tree (§25.2.3).
pub const MEMBER_ANNOTATIONS: &[&str] = &["peru", "lekapothe_kotthadhi", "dhaachu"];

/// Field and variant annotations (Annex W.10), not implemented yet.
pub const PLANNED_MEMBER_ANNOTATIONS: &[&str] = &["daari", "dorakaledhu"];

/// Standard traits of Annex W that `@automatic` will derive.
pub const PLANNED_TRAITS: &[&str] = &["web.Daari"];

/// Traits written in "Ante" (`std/ante/prelude.ante`) that `@automatic`
/// derives by writing their implementation as "Ante" source (§25.2).
pub const SOURCE_DERIVED_TRAITS: &[&str] = &["Raayochu", "Chadhavochu", "Kotthadhi"];

pub struct StdModule {
    pub name: &'static str,
    pub values: &'static [&'static str],
    pub types: &'static [&'static str],
    /// Generated bindings (`torch`, `python`): every member name is accepted.
    pub open: bool,
}

pub const STD_MODULES: &[StdModule] = &[
    StdModule {
        name: "lekkalu",
        values: &[
            "chuttu_kalupu",
            "chuttu_theesey",
            "chuttu_guninchu",
            "trupthiga_kalupu",
            "trupthiga_theesey",
            "trupthiga_guninchu",
            "kudhirithe_kalupu",
            "kudhirithe_theesey",
            "kudhirithe_guninchu",
            "kudhirithe_panchu",
            "sqrt",
            "sin",
            "cos",
            "tan",
            "exp",
            "log",
            "abs",
            "min",
            "max",
            "pow",
            "floor",
            "ceil",
            "round",
            "PI",
            "E",
            "INF",
            "NAN",
            "motham",
            "dot",
            "polchu_amsam",
        ],
        types: &[],
        open: false,
    },
    StdModule {
        name: "file",
        values: &[
            "chadhuvu",
            "raayi",
            "baitlu_chadhuvu",
            "baitlu_raayi",
            "undhaa",
            "folderaa",
            "list_cheyyi",
            "folder_cheyyi",
            "theesey",
            "path_kalupu",
            "path_peru",
            "path_folder",
            "path_extension",
        ],
        types: &["FileThappu"],
        open: false,
    },
    StdModule {
        name: "system",
        values: &[
            "args",
            "env",
            "line_chadhuvu",
            "flush",
            "vellipo",
            "nadupu",
            "os",
            "arch",
            "_release",
            "_env_text",
        ],
        types: &["Phalitham", "SystemThappu"],
        open: false,
    },
    StdModule {
        name: "samayam",
        values: &["ippudu", "timer", "_utc_nanosec"],
        types: &["Kshanam", "Kaalam"],
        open: false,
    },
    StdModule {
        name: "memory",
        values: &["allocate", "release", "reallocate", "copy", "sunna_cheyyi"],
        types: &[],
        open: false,
    },
    StdModule {
        name: "task",
        values: &["kalisi", "nidrapo", "cancel_ayyindhaa", "cpu_lu", "kaluva", "edhi_mundhu", "cancel_ready"],
        types: &[
            "Jattu",
            "Token",
            "TrapVivaram",
            "TaskThappu",
            "Thaalam",
            "Key",
            "Virigindhi",
            "Atomic",
            "Pampey",
            "Andhuko",
            "Moosesaaru",
            "Edhuruchoopu",
        ],
        open: false,
    },
    // Modules written wholly in "Ante" (`ANTE_MODULES`), apart from the
    // intrinsics their signature files declare.
    StdModule { name: "encoding", values: &[], types: &[], open: false },
    StdModule { name: "json", values: &[], types: &[], open: false },
    StdModule { name: "form", values: &[], types: &[], open: false },
    StdModule { name: "query", values: &[], types: &[], open: false },
    StdModule {
        name: "bhadram",
        values: &[
            "sha256",
            "sha512",
            "blake3",
            "random_baitlu",
            "okkatenaa",
            "_argon2id",
            "_aes_gcm_seal",
            "_aes_gcm_open",
        ],
        types: &[],
        open: false,
    },
    StdModule { name: "random", values: &[], types: &[], open: false },
    StdModule { name: "log", values: &[], types: &[], open: false },
    StdModule { name: "config", values: &[], types: &[], open: false },
    StdModule { name: "torch", values: &[], types: &[], open: true },
    StdModule { name: "python", values: &[], types: &[], open: true },
];
