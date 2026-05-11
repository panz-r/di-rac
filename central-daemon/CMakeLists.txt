cmake_minimum_required(VERSION 3.10)
project(divrr-central-daemon C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wall -Wextra -O3 -D_GNU_SOURCE")

include_directories(include)

# Enable ctest for central-daemon's own tests
enable_testing()

# Integrate Draugr (High-performance Hash Table)
# Suppress -Werror for draugr subdirectory so its test files (which are not
# cleaned up yet) don't break the central-daemon build. Core daemon targets
# (divrr-central-daemon, trie_test, broker_test) are unaffected.
add_compile_options(-Wno-error)
add_subdirectory(../draugr draugr_build)
include_directories(../draugr/include)

# Main Daemon executable
add_executable(divrr-central-daemon
    src/main.c
    src/trie.c
)

target_link_libraries(divrr-central-daemon pthread draugr)

# Test executable
add_executable(trie_test tests/trie_test.c src/trie.c)
target_link_libraries(trie_test pthread draugr)
add_test(NAME trie_test COMMAND trie_test)

add_executable(broker_test tests/broker_test.c)
target_link_libraries(broker_test pthread)
add_test(NAME broker_test COMMAND broker_test)
