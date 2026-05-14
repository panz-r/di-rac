pub mod bash;
pub mod c;
pub mod cpp;
pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;
pub mod java;
pub mod csharp;
pub mod ruby;
pub mod php;

use crate::language::Language;

pub struct LanguageQueries {
    pub symbol_query: &'static str,
    pub import_query: &'static str,
    pub call_query: &'static str,
}

pub fn get_queries(language: Language) -> LanguageQueries {
    match language {
        Language::Python => LanguageQueries {
            symbol_query: python::SYMBOL_QUERY,
            import_query: python::IMPORT_QUERY,
            call_query: python::CALL_QUERY,
        },
        Language::TypeScript | Language::JavaScript => LanguageQueries {
            symbol_query: typescript::SYMBOL_QUERY,
            import_query: typescript::IMPORT_QUERY,
            call_query: typescript::CALL_QUERY,
        },
        Language::C => LanguageQueries {
            symbol_query: c::SYMBOL_QUERY,
            import_query: c::IMPORT_QUERY,
            call_query: c::CALL_QUERY,
        },
        Language::Cpp => LanguageQueries {
            symbol_query: cpp::SYMBOL_QUERY,
            import_query: cpp::IMPORT_QUERY,
            call_query: cpp::CALL_QUERY,
        },
        Language::Rust => LanguageQueries {
            symbol_query: rust::SYMBOL_QUERY,
            import_query: rust::IMPORT_QUERY,
            call_query: rust::CALL_QUERY,
        },
        Language::Go => LanguageQueries {
            symbol_query: go::SYMBOL_QUERY,
            import_query: go::IMPORT_QUERY,
            call_query: go::CALL_QUERY,
        },
        Language::Bash => LanguageQueries {
            symbol_query: bash::SYMBOL_QUERY,
            import_query: bash::IMPORT_QUERY,
            call_query: bash::CALL_QUERY,
        },
        Language::Java => LanguageQueries {
            symbol_query: java::SYMBOL_QUERY,
            import_query: java::IMPORT_QUERY,
            call_query: java::CALL_QUERY,
        },
        Language::CSharp => LanguageQueries {
            symbol_query: csharp::SYMBOL_QUERY,
            import_query: csharp::IMPORT_QUERY,
            call_query: csharp::CALL_QUERY,
        },
        Language::Ruby => LanguageQueries {
            symbol_query: ruby::SYMBOL_QUERY,
            import_query: ruby::IMPORT_QUERY,
            call_query: ruby::CALL_QUERY,
        },
        Language::Php => LanguageQueries {
            symbol_query: php::SYMBOL_QUERY,
            import_query: php::IMPORT_QUERY,
            call_query: php::CALL_QUERY,
        },
    }
}
