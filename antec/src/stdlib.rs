//! Names the prelude and the standard library modules export (spec §8.5,
//! §13.5, §21). Until the standard library ships as "Ante" source (M10),
//! these tables stand in for it during name resolution.

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
    "khali",
    "raadhu",
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
    "size_of",
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
pub const ANNOTATIONS: &[&str] = &["pariksha", "twaraga", "automatic", "bayata", "C"];

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
pub const PLANNED_ANNOTATIONS: &[&str] = &["server", "theliyanivi_vadhiley"];

/// Field and variant annotations (§25.2.3, Annex W.10), not implemented yet.
pub const PLANNED_MEMBER_ANNOTATIONS: &[&str] =
    &["peru", "lekapothe_kotthadhi", "dhaachu", "daari", "dorakaledhu"];

/// Standard traits of Part IX and Annex W that `@automatic` will derive.
pub const PLANNED_TRAITS: &[&str] =
    &["Raayochu", "Chadhavochu", "Kotthadhi", "Pampochu", "Panchukovochu", "web.Daari"];

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
        values: &["args", "env", "line_chadhuvu", "flush", "vellipo", "nadupu", "os", "arch"],
        types: &["Phalitham", "SystemThappu"],
        open: false,
    },
    StdModule { name: "samayam", values: &["ippudu"], types: &["Kshanam", "Kaalam"], open: false },
    StdModule {
        name: "memory",
        values: &["allocate", "release", "reallocate", "copy"],
        types: &[],
        open: false,
    },
    StdModule { name: "torch", values: &[], types: &[], open: true },
    StdModule { name: "python", values: &[], types: &[], open: true },
];
