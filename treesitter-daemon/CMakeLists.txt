cmake_minimum_required(VERSION 3.10)
project(di-rvv-analyzer C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wall -Wextra -O3 -D_GNU_SOURCE")

# --- Dependencies ---

# 1. Core Tree-Sitter
set(TS_DIR ${CMAKE_CURRENT_SOURCE_DIR}/vendor/tree-sitter)
include_directories(${TS_DIR}/lib/include)
include_directories(${TS_DIR}/lib/src)

add_library(tree-sitter STATIC
    ${TS_DIR}/lib/src/lib.c
)

# 2. SQLite3 (Bundled Amalgamation)
set(SQLITE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/vendor/sqlite)
include_directories(${SQLITE_DIR})

add_library(sqlite STATIC
    ${SQLITE_DIR}/sqlite3.c
)
target_link_libraries(sqlite m dl)

# 3. Pthread
find_package(Threads REQUIRED)

# --- Grammars ---

macro(add_grammar LANG DIR)
    set(SRC ${DIR}/src/parser.c)
    if(EXISTS ${DIR}/src/scanner.c)
        list(APPEND SRC ${DIR}/src/scanner.c)
    endif()
    
    add_library(ts-grammar-${LANG} STATIC ${SRC})
    target_include_directories(ts-grammar-${LANG} PRIVATE ${DIR}/src)
    target_include_directories(ts-grammar-${LANG} PRIVATE ${TS_DIR}/lib/include)
    set_target_properties(ts-grammar-${LANG} PROPERTIES POSITION_INDEPENDENT_CODE ON)
endmacro()

set(VENDOR_DIR ${CMAKE_CURRENT_SOURCE_DIR}/vendor)

add_grammar(python ${VENDOR_DIR}/tree-sitter-python)
add_grammar(typescript ${VENDOR_DIR}/tree-sitter-typescript/typescript)
add_grammar(c ${VENDOR_DIR}/tree-sitter-c)
add_grammar(cpp ${VENDOR_DIR}/tree-sitter-cpp)
add_grammar(rust ${VENDOR_DIR}/tree-sitter-rust)
add_grammar(go ${VENDOR_DIR}/tree-sitter-go)
add_grammar(bash ${VENDOR_DIR}/tree-sitter-bash)
add_grammar(java ${VENDOR_DIR}/tree-sitter-java)
add_grammar(csharp ${VENDOR_DIR}/tree-sitter-c-sharp)
add_grammar(ruby ${VENDOR_DIR}/tree-sitter-ruby)
add_grammar(php ${VENDOR_DIR}/tree-sitter-php/php)

# --- Daemon ---

include_directories(include-c)
include_directories(../command-daemon/src) # Reuse JSON helpers

set(DAEMON_SOURCES
    src-c/main.c
    src-c/analyzer.c
    src-c/languages.c
    src-c/queries.c
    src-c/skeleton.c
    src-c/repo_map.c
    src-c/search.c
    src-c/db.c
)

add_executable(di-rvv-analyzer ${DAEMON_SOURCES})

set(LIBS
    tree-sitter
    sqlite
    ts-grammar-python
    ts-grammar-typescript
    ts-grammar-c
    ts-grammar-cpp
    ts-grammar-rust
    ts-grammar-go
    ts-grammar-bash
    ts-grammar-java
    ts-grammar-csharp
    ts-grammar-ruby
    ts-grammar-php
    Threads::Threads
    m
    dl
)

target_link_libraries(di-rvv-analyzer ${LIBS})

# --- Tests ---

add_executable(test_db
    tests/test_db.c
    src-c/db.c
    src-c/analyzer.c
    src-c/languages.c
    src-c/queries.c
)
target_link_libraries(test_db ${LIBS})

install(TARGETS di-rvv-analyzer DESTINATION bin)
