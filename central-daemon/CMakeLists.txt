cmake_minimum_required(VERSION 3.10)
project(divrr-central-daemon C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wall -Wextra -O3 -D_GNU_SOURCE")

include_directories(include)

# Enable ctest for central-daemon's own tests
enable_testing()

# Build Draugr as a separate library (not a subdirectory)
add_library(draugr STATIC IMPORTED)
set_target_properties(draugr PROPERTIES
    IMPORTED_LOCATION ${CMAKE_SOURCE_DIR}/../draugr/build/libdraugr.a
    INTERFACE_INCLUDE_DIRECTORIES ${CMAKE_SOURCE_DIR}/../draugr/include
)

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
