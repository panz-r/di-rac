cmake_minimum_required(VERSION 3.16)
project(dirac-cmd C)

set(CMAKE_C_STANDARD 17)
set(CMAKE_C_STANDARD_REQUIRED ON)

add_executable(dirac-cmd
    src/main.c
    src/protocol.c
    src/executor.c
    src/session.c
)

target_include_directories(dirac-cmd PRIVATE src)

# Release flags
if(CMAKE_BUILD_TYPE STREQUAL "Release")
    target_compile_options(dirac-cmd PRIVATE -O2 -Wall -Wextra)
endif()
