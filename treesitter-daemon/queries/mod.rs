pub mod bash;
pub mod c;
pub mod cpp;
pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

use crate::language::Language;

pub struct LanguageQueries {
    pub symbol_query: &'static str,
    pub import_query: &'static str,
}

pub fn get_queries(language: Language) -> LanguageQueries {
    match language {
        Language::Python => LanguageQueries {
            symbol_query: python::SYMBOL_QUERY,
            import_query: python::IMPORT_QUERY,
        },
        Language::TypeScript | Language::JavaScript => LanguageQueries {
            symbol_query: typescript::SYMBOL_QUERY,
            import_query: typescript::IMPORT_QUERY,
        },
        Language::C => LanguageQueries {
            symbol_query: c::SYMBOL_QUERY,
            import_query: c::IMPORT_QUERY,
        },
        Language::Cpp => LanguageQueries {
            symbol_query: cpp::SYMBOL_QUERY,
            import_query: cpp::IMPORT_QUERY,
        },
        Language::Rust => LanguageQueries {
            symbol_query: rust::SYMBOL_QUERY,
            import_query: rust::IMPORT_QUERY,
        },
        Language::Go => LanguageQueries {
            symbol_query: go::SYMBOL_QUERY,
            import_query: go::IMPORT_QUERY,
        },
        Language::Bash => LanguageQueries {
            symbol_query: bash::SYMBOL_QUERY,
            import_query: bash::IMPORT_QUERY,
        },
    }
}
