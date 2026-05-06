cmake_minimum_required(VERSION 3.10)
project(di-vrr-central-deamon C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wall -Wextra -O3 -D_GNU_SOURCE")

include_directories(include)

# Integrate Draugr (High-performance Hash Table)
add_subdirectory(../draugr draugr_build)
include_directories(../draugr/include)

# Main Daemon executable
add_executable(di-vrr-central-deamon 
    src/main.c 
    src/trie.c
)

target_link_libraries(di-vrr-central-deamon pthread draugr)

# Test executable
add_executable(trie_test tests/trie_test.c src/trie.c)
target_link_libraries(trie_test pthread draugr)

add_executable(broker_test tests/broker_test.c)
target_link_libraries(broker_test pthread)
