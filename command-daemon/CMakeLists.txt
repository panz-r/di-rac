cmake_minimum_required(VERSION 3.16)
project(di-rvv-cmd C)

set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)

add_executable(di-rvv-cmd
    src/main.c
    src/protocol.c
    src/executor.c
    src/session.c
    src/safety.c
)

target_include_directories(di-rvv-cmd PRIVATE src)

# Release flags
if(CMAKE_BUILD_TYPE STREQUAL "Release")
    target_compile_options(di-rvv-cmd PRIVATE -O2 -Wall -Wextra)
endif()
